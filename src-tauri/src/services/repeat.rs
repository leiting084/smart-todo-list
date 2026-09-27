//! 重复待办（F15/T3.5）：规则 CRUD + 实例生成（每日/每周/每月）+ 自动延期。
//!
//! 0002 表：todo_repeat_rules（模板）→ todo_repeat_items（实例，UNIQUE(todo_id,date)）。
//! - 生成窗口：以今天为锚，生成 **未来 7 天 + 补过去 1 天** 的实例（重复串行时天/月不重叠）；
//! - weekly：days_json 存 [0-6]（0=周日），空=全部；
//! - **monthly 月末漂移**：31 号规则在 2 月落到月末（28/29）；每天循环按真实日历 NaiveDate 推进；
//! - 幂等：INSERT OR IGNORE（重启/重复生成不产生重复）。
//! - postpone_auto：非重复待办过点未完成顺延（0 点后把 date=昨天的未完非重复待办 → date=今天）。

use chrono::{Datelike, Duration, Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::State;

use crate::db::Db;
use crate::id::{new_id, ymd_of};
use crate::models::Todo;

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepeatRule {
    pub id: String,
    pub todo_id: String,
    /// daily / weekly / monthly
    pub freq: String,
    /// weekly: [0-6]（0=周日）；monthly: [day]；daily: 忽略
    pub days: Vec<u8>,
    pub start_date: i64,
    pub end_date: Option<i64>,
}

// ---------- 规则 CRUD ----------

pub fn set_rule(
    conn: &Connection,
    todo_id: &str,
    freq: &str,
    days: &[u8],
    start_date: i64,
    end_date: Option<i64>,
) -> Result<RepeatRule, String> {
    if !["daily", "weekly", "monthly"].contains(&freq) {
        return Err(format!("freq 必须是 daily/weekly/monthly，实际 {freq}"));
    }
    // 校验语义按频分：weekly=0-6 星期；monthly=1-31 日（月末自动漂移）
    if freq == "monthly" {
        if days.iter().any(|d| *d == 0 || *d > 31) {
            return Err("monthly 的 days 应为 1-31（月内日）".into());
        }
    } else if days.iter().any(|d| *d > 6) {
        return Err("weekly 的 days 元素必须 0-6（0=周日）".into());
    }
    // 待办必须存在
    conn.query_row("SELECT 1 FROM todos WHERE id=?1", params![todo_id], |_| Ok(()))
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("待办不存在：{todo_id}"))?;

    let days_json = serde_json::to_string(days).map_err(|e| e.to_string())?;
    let id = match conn
        .query_row(
            "SELECT id FROM todo_repeat_rules WHERE todo_id=?1",
            params![todo_id],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
    {
        Some(id) => id,
        None => {
            let id = new_id();
            conn.execute(
                "INSERT INTO todo_repeat_rules (id,todo_id,freq,days_json,start_date,end_date,created_at)
                 VALUES (?1,?2,'daily','[]',0,NULL,?3)",
                params![id, todo_id, now_ms()],
            )
            .map_err(|e| e.to_string())?;
            id
        }
    };
    conn.execute(
        "UPDATE todo_repeat_rules SET freq=?2, days_json=?3, start_date=?4, end_date=?5 WHERE id=?1",
        params![id, freq, days_json, start_date, end_date],
    )
    .map_err(|e| e.to_string())?;
    get_rule(conn, todo_id)?.ok_or_else(|| "规则读取失败".into())
}

pub fn get_rule(conn: &Connection, todo_id: &str) -> Result<Option<RepeatRule>, String> {
    let r: Option<(String, String, String, i64, Option<i64>)> = conn
        .query_row(
            "SELECT id, freq, days_json, start_date, end_date FROM todo_repeat_rules WHERE todo_id=?1",
            params![todo_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(r.map(|(id, freq, days_json, start_date, end_date)| RepeatRule {
        id,
        todo_id: todo_id.to_string(),
        freq,
        days: serde_json::from_str(&days_json).unwrap_or_default(),
        start_date,
        end_date,
    }))
}

pub fn clear_rule(conn: &Connection, todo_id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM todo_repeat_rules WHERE todo_id=?1", params![todo_id])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---------- 实例生成 ----------

/// 规则在某天是否应生成（含月末漂移：monthly 规则 day>当月天数 → 落到月末）。
fn rule_occurs(rule: &RepeatRule, date: &NaiveDate) -> bool {
    let ymd = ymd_of(date.and_hms_opt(0, 0, 0).unwrap());
    if ymd < rule.start_date {
        return false;
    }
    if let Some(end) = rule.end_date {
        if ymd > end {
            return false;
        }
    }
    match rule.freq.as_str() {
        "daily" => true,
        "weekly" => {
            let dow = date.weekday().num_days_from_sunday() as u8;
            if rule.days.is_empty() {
                true
            } else {
                rule.days.contains(&dow)
            }
        }
        "monthly" => {
            let target = if rule.days.is_empty() { 1 } else { rule.days[0] };
            let last = last_day_of_month(date.year(), date.month()) as u8;
            let day = date.day() as u8;
            // 目标日 = 月底就漂移到当月最后一天
            let wanted = target.min(last);
            day == wanted
        }
        _ => false,
    }
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    // 下月 1 号回退一天
    let (y2, m2) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(y2, m2, 1)
        .unwrap()
        .pred_opt()
        .unwrap()
        .day()
}

/// 生成未来 7 天 + 补过去 1 天实例（powered by today）。返回新增条数。
pub fn generate_instances(conn: &Connection, today: NaiveDate) -> Result<usize, String> {
    let rules: Vec<(String, String, String, i64, Option<i64>)> = {
        let mut stmt = conn
            .prepare("SELECT id, freq, days_json, start_date, end_date FROM todo_repeat_rules")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(|e| e.to_string())?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r.map_err(|e| e.to_string())?);
        }
        v
    };
    let rules: Vec<RepeatRule> = rules
        .into_iter()
        .map(|(id, freq, days_json, start_date, end_date)| RepeatRule {
            id,
            todo_id: String::new(),
            freq,
            days: serde_json::from_str(&days_json).unwrap_or_default(),
            start_date,
            end_date,
        })
        .collect();

    let mut added = 0;
    // 生成窗口：today-1 .. today+7
    for rule in &rules {
        for off in -1..=7i64 {
            let date = today + Duration::days(off);
            if !rule_occurs(rule, &date) {
                continue;
            }
            let ymd = ymd_of(date.and_hms_opt(0, 0, 0).unwrap());
            let n = conn
                .execute(
                    "INSERT OR IGNORE INTO todo_repeat_items (id, todo_id, date, completed, completed_at)
                     VALUES (?1, ?2, ?3, 0, NULL)",
                    params![new_id(), todo_id_of(conn, &rule.id)?, ymd],
                )
                .map_err(|e| e.to_string())?;
            added += n;
        }
    }
    let _ = now_ms();
    Ok(added)
}

fn todo_id_of(conn: &Connection, rule_id: &str) -> Result<String, String> {
    conn.query_row("SELECT todo_id FROM todo_repeat_rules WHERE id=?1", params![rule_id], |r| r.get(0))
        .map_err(|e| e.to_string())
}

/// 自动延期：date=昨天(或更早) 且未完成 且 非重复模板 的待办 → date=今天。
pub fn postpone_overdue(conn: &Connection, today: NaiveDate) -> Result<usize, String> {
    let today_ymd = ymd_of(today.and_hms_opt(0, 0, 0).unwrap());
    // 仅元数据非重复的 todo（无 parent 且非 repeat 模板；postpone_auto 各列在模板 todo 上）
    let n = conn
        .execute(
            "UPDATE todos SET date=?2, updated_at=?3
             WHERE completed=0 AND date IS NOT NULL AND date<?2
               AND postpone_auto=1 AND id NOT IN (SELECT todo_id FROM todo_repeat_rules)",
            params![today_ymd, today_ymd, now_ms()],
        )
        .map_err(|e| e.to_string())?;
    Ok(n)
}

// ---------- 读取路径（F15/F14 修复：实例并入今日/月总览/提醒） ----------
//
// 历史坑：todo_repeat_items 只被写入，list_todos / month_stats / due_todos 全部只查 todos，
// 导致重复待办既不显示也不提醒。下列函数把实例“渲染”成普通 Todo（id=实例 id，
// 标题/分类/优先级/颜色/项目/时间取自模板，completed/date/completed_at 取自实例行），
// 供三条读取路径复用；完成操作按实例 id 路由回 todo_repeat_items（见 commands::todo）。

/// YYYYMMDD → NaiveDate（非法返回 None）。
fn ymd_to_naive(ymd: i64) -> Option<NaiveDate> {
    let (y, m, d) = ((ymd / 10000) as i32, ((ymd / 100) % 100) as u32, (ymd % 100) as u32);
    NaiveDate::from_ymd_opt(y, m, d)
}

/// 实例视图行的公共 SELECT 片段（inst 别名 + 模板 t 别名）。
const INSTANCE_COLUMNS: &str =
    "inst.id, t.title, t.content, inst.completed, inst.date, t.sort_order, t.color, \
     t.category, t.priority, t.project_id, t.time, t.parent_id, 0 AS postpone_auto, \
     t.created_at, t.updated_at, inst.completed_at, t.id AS template_id";

fn map_instance_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Todo, String)> {
    let todo = Todo {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        completed: row.get::<_, i64>(3)? != 0,
        date: row.get(4)?,
        sort_order: row.get(5)?,
        color: row.get(6)?,
        category: row.get(7)?,
        priority: row.get(8)?,
        project_id: row.get(9)?,
        time: row.get(10)?,
        parent_id: row.get(11)?,
        postpone_auto: false,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        completed_at: row.get(14)?,
        tags: Vec::new(),
    };
    Ok((todo, row.get(16)?))
}

/// 按一组 (Todo, template_id) 批量回填标签（标签挂在模板 todo 上，实例继承）。
fn fill_instance_tags(conn: &Connection, items: &mut [(Todo, String)]) -> Result<(), String> {
    if items.is_empty() {
        return Ok(());
    }
    let tpl_ids: Vec<String> = items.iter().map(|(_, t)| t.clone()).collect();
    let ph = tpl_ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT it.item_id, tg.name FROM item_tags it JOIN tags tg ON tg.id = it.tag_id
         WHERE it.item_type='todo' AND it.item_id IN ({ph}) ORDER BY tg.name"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(tpl_ids), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for r in rows {
        let (tid, name) = r.map_err(|e| e.to_string())?;
        map.entry(tid).or_default().push(name);
    }
    for (todo, tpl) in items.iter_mut() {
        if let Some(v) = map.get(tpl) {
            todo.tags = v.clone();
        }
    }
    Ok(())
}

/// 某日的全部重复实例（渲染成 Todo）。`completed` = Some(true/false) 追加完成过滤，None 全给。
/// 结果按 sort_order、实例 id 稳定排序。
pub fn instances_for_date(
    conn: &Connection,
    date: i64,
    completed: Option<bool>,
) -> Result<Vec<Todo>, String> {
    let sql = format!(
        "SELECT {INSTANCE_COLUMNS}
         FROM todo_repeat_items inst JOIN todos t ON t.id = inst.todo_id
         WHERE inst.date = ?1 AND (?2 = -1 OR inst.completed = ?2)
         ORDER BY t.sort_order ASC, inst.id ASC"
    );
    let bound = completed.map_or(-1i64, |c| c as i64);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![date, bound], map_instance_row)
        .map_err(|e| e.to_string())?;
    let mut items: Vec<(Todo, String)> = Vec::new();
    for r in rows {
        items.push(r.map_err(|e| e.to_string())?);
    }
    // items 已是 Vec<(Todo, String)>，直接接管所有权即可（clippy: map_identity）
    let mut owned: Vec<(Todo, String)> = items;
    fill_instance_tags(conn, &mut owned)?;
    Ok(owned.into_iter().map(|(t, _)| t).collect())
}

/// 按实例 id 取单条实例（渲染成 Todo）；非实例返回 None。供完成后回读视图行。
pub fn get_instance_todo(conn: &Connection, inst_id: &str) -> Result<Option<Todo>, String> {
    let sql = format!(
        "SELECT {INSTANCE_COLUMNS}
         FROM todo_repeat_items inst JOIN todos t ON t.id = inst.todo_id
         WHERE inst.id = ?1"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params![inst_id], map_instance_row)
        .map_err(|e| e.to_string())?;
    match rows.next() {
        Some(r) => {
            let (todo, tpl) = r.map_err(|e| e.to_string())?;
            let mut one = vec![(todo, tpl)];
            fill_instance_tags(conn, &mut one)?;
            Ok(Some(one.into_iter().next().unwrap().0))
        }
        None => Ok(None),
    }
}

/// id 是否为重复实例（todo_repeat_items 主键命中）。供 set_completed 路由。
pub fn is_repeat_instance(conn: &Connection, id: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM todo_repeat_items WHERE id = ?1)",
        params![id],
        |r| r.get::<_, i64>(0),
    )
    .map(|v| v != 0)
    .map_err(|e| e.to_string())
}

/// 完成/取消一条实例（协作语义简化：实例整体勾选，不动模板）。
pub fn set_instance_completed(conn: &Connection, inst_id: &str, completed: bool) -> Result<(), String> {
    let now = now_ms();
    let completed_at: Option<i64> = if completed { Some(now) } else { None };
    let n = conn
        .execute(
            "UPDATE todo_repeat_items SET completed = ?2, completed_at = ?3 WHERE id = ?1",
            params![inst_id, completed as i64, completed_at],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("重复实例不存在：{inst_id}"));
    }
    Ok(())
}

/// 幂等确保某日的实例已生成（视图/提醒读取前自愈，避免"当日没跨点就没生成"）。
/// 以 date 为锚生成 [date-1, date+7] 窗口，INSERT OR IGNORE 天然幂等。
pub fn ensure_instances_for_date(conn: &Connection, date: i64) -> Result<(), String> {
    if let Some(anchor) = ymd_to_naive(date) {
        // 生成失败（无规则/日期非法）不影响读取：吞掉新增数即可。
        let _ = generate_instances(conn, anchor);
    }
    Ok(())
}

// ---------- Tauri 命令 ----------

fn today() -> NaiveDate {
    Local::now().date_naive()
}

#[tauri::command]
pub fn repeat_set(
    db: State<'_, Db>,
    todo_id: String,
    freq: String,
    days: Vec<u8>,
    start_date: i64,
    end_date: Option<i64>,
) -> Result<RepeatRule, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    set_rule(&conn, &todo_id, &freq, &days, start_date, end_date)
}

#[tauri::command]
pub fn repeat_get(db: State<'_, Db>, todo_id: String) -> Result<Option<RepeatRule>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    get_rule(&conn, &todo_id)
}

#[tauri::command]
pub fn repeat_clear(db: State<'_, Db>, todo_id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    clear_rule(&conn, &todo_id)
}

/// 调用时机：启动、0 点转点（前端今日视图轮询时调）。
#[tauri::command]
pub fn repeat_generate(db: State<'_, Db>) -> Result<usize, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    generate_instances(&conn, today())
}

#[tauri::command]
pub fn repeat_postpone(db: State<'_, Db>) -> Result<usize, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    postpone_overdue(&conn, today())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::todo::create_todo;
    use crate::models::CreateTodo;
    use std::path::PathBuf;

    fn temp_db(tag: &str) -> (Connection, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-repeat-test-{}-{}-{}",
            std::process::id(),
            std::line!(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&root);
        let bkp = root.join("backups");
        std::fs::create_dir_all(&bkp).unwrap();
        let conn = crate::db::open_at(&root.join("todolist.db"), &bkp).unwrap();
        (conn, root)
    }

    fn mk(conn: &Connection, title: &str) -> String {
        create_todo(conn, CreateTodo {
            title: title.into(),
            content: None,
            date: Some(20260918),
            color: None,
            category: None,
            priority: None,
            project_id: None,
            time: None,
            parent_id: None,
            sort_order: None,
        })
        .unwrap()
        .id
    }

    fn count_items(conn: &Connection, todo_id: &str, date: i64) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM todo_repeat_items WHERE todo_id=?1 AND date=?2",
            params![todo_id, date],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// TASKS 验收：周重复在 7 天内生成正确星期的实例 + 幂等
    #[test]
    fn weekly_generates_correct_days_idempotent() {
        let (conn, root) = temp_db("weekly");
        let conn = &conn;
        let tid = mk(conn, "周一周四任务");
        // 2026-09-18 是周五；窗口 09-17(四) .. 09-25(五)。周一=09-21，周四=09-24
        set_rule(conn, &tid, "weekly", &[1, 4], 20260901, None).unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        // 窗口 today-1 .. today+7 = 09-17(四)..09-25(五)；[周一,周四] → 09-17(四)+09-21(一)+09-24(四)
        let added = generate_instances(conn, today).unwrap();
        assert_eq!(added, 3, "窗口内应 09-17(四)+09-21(一)+09-24(四)");

        let thu17 = ymd_of(NaiveDate::from_ymd_opt(2026, 9, 17).unwrap().and_hms_opt(0, 0, 0).unwrap());
        let mon = ymd_of(NaiveDate::from_ymd_opt(2026, 9, 21).unwrap().and_hms_opt(0, 0, 0).unwrap());
        let thu = ymd_of(NaiveDate::from_ymd_opt(2026, 9, 24).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(count_items(conn, &tid, thu17), 1, "补过去 1 天的周四");
        assert_eq!(count_items(conn, &tid, mon), 1);
        assert_eq!(count_items(conn, &tid, thu), 1);
        // 非周一/周四不应有
        let sun = ymd_of(NaiveDate::from_ymd_opt(2026, 9, 20).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(count_items(conn, &tid, sun), 0);

        // 幂等：重复生成不产生重复（INSERT OR IGNORE）
        let again = generate_instances(conn, today).unwrap();
        assert_eq!(again, 0);
        assert_eq!(count_items(conn, &tid, mon), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    /// TASKS 验收：31 号月重复在 2 月落到 28/29（月末漂移）
    #[test]
    fn monthly_31_drifts_to_month_end() {
        let (conn, root) = temp_db("monthly");
        let conn = &conn;
        let tid = mk(conn, "每月31日");
        set_rule(conn, &tid, "monthly", &[31], 20260101, None).unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(); // 窗口 02-27..03-07
        generate_instances(conn, today).unwrap();
        // 2 月无 31 号 → 落到 02-28；3 月有 31 号 → 03-31 不在窗口（窗口到 03-07）→ 无
        let feb28 = ymd_of(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(count_items(conn, &tid, feb28), 1, "2 月 31 号规则应落到月末 28");
        // 2028 闰年 2 月 → 29
        clear_rule(conn, &tid).unwrap();
        let tid2 = mk(conn, "闰年31");
        set_rule(conn, &tid2, "monthly", &[31], 20280101, None).unwrap();
        let today28 = NaiveDate::from_ymd_opt(2028, 2, 28).unwrap();
        generate_instances(conn, today28).unwrap();
        // 窗口 2028-02-27..03-06；闰年 2 月 29（31 号月末漂移），3 月 31 不在窗口
        let feb29 = ymd_of(NaiveDate::from_ymd_opt(2028, 2, 29).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(count_items(conn, &tid2, feb29), 1, "闰年 2 月 29 月末漂移");
        let _ = std::fs::remove_dir_all(root);
    }

    /// daily + 未达 start 不生成 + clear 后停
    #[test]
    fn daily_and_clear() {
        let (conn, root) = temp_db("daily");
        let conn = &conn;
        let tid = mk(conn, "每日");
        set_rule(conn, &tid, "daily", &[], 20260920, None).unwrap(); // 起点 09-20
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        // 窗口 09-17..09-25；start 之后 = 09-20..09-25 = 6 天
        assert_eq!(generate_instances(conn, today).unwrap(), 6, "start(09-20) 之后 6 天");

        clear_rule(conn, &tid).unwrap();
        assert!(get_rule(conn, &tid).unwrap().is_none());
        // 清规则后旧实例仍在（历史保留），不再新增
        assert_eq!(generate_instances(conn, today).unwrap(), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 自动延期：昨天未完非重复待办 → 今天；完成/重复模板不延
    #[test]
    fn postpone_overdue_rules() {
        let (conn, root) = temp_db("postpone");
        let conn = &conn;
        // 昨天未完 + postpone_auto=1
        let a = mk(conn, "顺延甲");
        conn.execute("UPDATE todos SET date=20260917, postpone_auto=1 WHERE id=?1", params![a]).unwrap();
        // 昨天未完但不顺延
        let b = mk(conn, "不顺延乙");
        conn.execute("UPDATE todos SET date=20260917, postpone_auto=0 WHERE id=?1", params![b]).unwrap();
        // 已完成的昨天
        let c = mk(conn, "已完成丙");
        conn.execute("UPDATE todos SET date=20260917, postpone_auto=1 WHERE id=?1", params![c]).unwrap();
        crate::commands::todo::set_completed(conn, &c, true).unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        postpone_overdue(conn, today).unwrap();
        let d: i64 = conn.query_row("SELECT date FROM todos WHERE id=?1", params![a], |r| r.get(0)).unwrap();
        assert_eq!(d, 20260918, "甲应顺延到今天");
        let d2: i64 = conn.query_row("SELECT date FROM todos WHERE id=?1", params![b], |r| r.get(0)).unwrap();
        assert_eq!(d2, 20260917, "乙不受影响（未开自动顺延）");
        let d3: i64 = conn.query_row("SELECT date FROM todos WHERE id=?1", params![c], |r| r.get(0)).unwrap();
        assert_eq!(d3, 20260917, "已完成不延");
        let _ = std::fs::remove_dir_all(root);
    }

    fn mk_at(conn: &Connection, title: &str, date: i64, time: Option<&str>) -> String {
        create_todo(conn, CreateTodo {
            title: title.into(),
            content: None,
            date: Some(date),
            color: None,
            category: None,
            priority: None,
            project_id: None,
            time: time.map(String::from),
            parent_id: None,
            sort_order: None,
        })
        .unwrap()
        .id
    }

    /// F15 回归（走真实读取路径）：每日重复待办在"今日"list_todos 里恰好出现一次（实例，非模板），
    /// 且勾选该实例只落实例完成状态、不污染模板。旧测试只数孤儿表行，掩盖了"读不到"的 bug。
    #[test]
    fn daily_instance_surfaces_in_today_and_completes_without_touching_template() {
        use crate::commands::todo::{get_todo, list_todos, set_completed};
        use crate::models::TodoFilter;
        let (conn, root) = temp_db("today_show");
        let conn = &conn;
        let tid = mk_at(conn, "每日打卡", 20260918, None);
        set_rule(conn, &tid, "daily", &[], 20260901, None).unwrap();
        let day = 20260918;

        let list = list_todos(conn, &TodoFilter { date: Some(day), ..Default::default() }).unwrap();
        let hits: Vec<_> = list.iter().filter(|t| t.title == "每日打卡").collect();
        assert_eq!(hits.len(), 1, "今日视图该重复项应恰好 1 条（实例），不得与模板同日重复");
        let inst = &hits[0];
        assert_ne!(inst.id, tid, "返回的应是实例 id，而非模板 id");
        assert!(!inst.completed);

        // 勾选实例 → 实例完成，模板整体仍未完成
        set_completed(conn, &inst.id, true).unwrap();
        assert!(!get_todo(conn, &tid).unwrap().unwrap().completed, "完成实例不得把模板整体标记完成");

        let list2 = list_todos(conn, &TodoFilter { date: Some(day), ..Default::default() }).unwrap();
        let hits2: Vec<_> = list2.iter().filter(|t| t.title == "每日打卡").collect();
        assert_eq!(hits2.len(), 1);
        assert!(hits2[0].completed, "重查今日，实例显示为已完成");
        let _ = std::fs::remove_dir_all(root);
    }

    /// F14 / §13.1 回归：重复待办的当日实例进 due_todos（键为实例 id，可去重），模板被排除不重复弹。
    #[test]
    fn reminder_includes_instance_and_excludes_template() {
        use crate::services::reminder::{due_todos, try_mark_fired};
        let (conn, root) = temp_db("remind_inst");
        let conn = &conn;
        let tid = mk_at(conn, "晨会", 20260918, Some("08:00"));
        set_rule(conn, &tid, "daily", &[], 20260901, None).unwrap();
        let day = 20260918;
        ensure_instances_for_date(conn, day).unwrap();

        let due = due_todos(conn, day, "00:00", "08:30").unwrap();
        assert!(!due.iter().any(|(id, _)| *id == tid), "模板不应出现在 due（防重复弹）");
        let inst_id = due
            .iter()
            .find(|(_, title)| title == "晨会")
            .map(|(id, _)| id.clone())
            .expect("实例应进 due 提醒");
        assert_ne!(inst_id, tid);
        // 去重：同一实例同一天只弹一次
        assert!(try_mark_fired(conn, &inst_id, day).unwrap());
        assert!(!try_mark_fired(conn, &inst_id, day).unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    /// F16 回归：月总览在"模板没被排的那天"也计入重复实例（证明重复真的进了日历），且不重复数模板。
    #[test]
    fn month_overview_counts_instance_on_non_template_days() {
        use crate::commands::dashboard::month_stats;
        let (conn, root) = temp_db("month_inst");
        let conn = &conn;
        let tid = mk_at(conn, "周报", 20260918, None);
        set_rule(conn, &tid, "daily", &[], 20260918, None).unwrap();
        ensure_instances_for_date(conn, 20260918).unwrap(); // 窗口 09-17..09-25

        let s = month_stats(conn, 2026, 9).unwrap();
        let cell = |d: i64| s.days.iter().find(|x| x.date == d).map(|x| x.total).unwrap_or(-1);
        // 09-20 是模板没有的日期，但 daily 实例应存在 → 该格 total=1
        assert_eq!(cell(20260920), 1, "非模板日也应计入重复实例（月历看到重复真的发生）");
        assert_eq!(cell(20260918), 1, "模板日只算实例一份，不双计模板");
        let _ = std::fs::remove_dir_all(root);
    }
}