//! 仪表板统计命令（F34，继承旧版）：纯 SQL 聚合，无第三方图表库。
//!
//! 口径（与旧版对齐）：
//! - 今日待办：date=今天 且未完成；
//! - 逾期：date<今天 且未完成；
//! - 本周完成：completed_at 落在本周一 00:00（本地）之后；
//! - 项目分布：未归档项目的待办总数/完成数（含历史）；
//! - 目标进度：活动目标的关联待办完成率；
//! - 近 7 天趋势：按 completed_at 的本地日期分桶（今天往前 6 天，缺日补 0）。

use chrono::{Datelike, Local, NaiveDate, NaiveTime};
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

use crate::db::Db;
use crate::id::ymd_of;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub today_count: i64,
    pub overdue_count: i64,
    pub week_completed: i64,
    pub project_distribution: Vec<ProjectSlice>,
    pub goal_progress: Vec<GoalSlice>,
    pub week_trend: Vec<TrendPoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSlice {
    pub project_id: String,
    pub name: String,
    pub color: Option<String>,
    pub total: i64,
    pub done: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSlice {
    pub goal_id: String,
    pub title: String,
    pub todo_count: i64,
    pub done_count: i64,
    pub progress: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendPoint {
    /// YYYYMMDD
    pub date: i64,
    pub completed: i64,
}

fn local_today_ymd() -> i64 {
    ymd_of(Local::now().naive_local())
}

/// 本周一 00:00 的本地时间戳（毫秒）。
fn week_start_ms() -> i64 {
    let now = Local::now().naive_local();
    let date = now.date();
    // chrono: monday=0（weekday().num_days_from_monday()）
    let monday = date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64);
    monday
        .and_time(NaiveTime::MIN)
        .and_local_timezone(Local)
        .single()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

pub fn dashboard_stats(conn: &Connection) -> Result<DashboardStats, String> {
    let today = local_today_ymd();

    let (today_count, overdue_count): (i64, i64) = conn
        .query_row(
            "SELECT
               COALESCE(SUM(CASE WHEN date=?1 THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN date<?1 THEN 1 ELSE 0 END),0)
             FROM todos WHERE completed=0",
            params![today],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;

    let week_start = week_start_ms();
    let week_completed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM todos WHERE completed=1 AND completed_at>=?1",
            params![week_start],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    // 项目分布（未归档项目；LEFT JOIN 保住空项目）
    let mut stmt = conn
        .prepare(
            "SELECT p.id, p.name, p.color, COUNT(t.id), COALESCE(SUM(t.completed),0)
             FROM projects p LEFT JOIN todos t ON t.project_id = p.id
             WHERE p.archived=0
             GROUP BY p.id ORDER BY p.sort_order, p.created_at",
        )
        .map_err(|e| e.to_string())?;
    let mut project_distribution = Vec::new();
    let rows = stmt
        .query_map([], |r| {
            Ok(ProjectSlice {
                project_id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                total: r.get(3)?,
                done: r.get::<_, i64>(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    for r in rows {
        project_distribution.push(r.map_err(|e| e.to_string())?);
    }

    // 目标进度（活动目标）：主进度=关联待办完成率（F32/§5）；无关联待办时回退手动进度（0005）
    let mut stmt = conn
        .prepare(
            "SELECT g.id, g.title, g.progress, COUNT(t.id), COALESCE(SUM(t.completed),0)
             FROM goals g
             LEFT JOIN goal_links gl ON gl.goal_id = g.id AND gl.item_type='todo'
             LEFT JOIN todos t ON t.id = gl.item_id
             WHERE g.status='active'
             GROUP BY g.id ORDER BY g.sort_order, g.created_at",
        )
        .map_err(|e| e.to_string())?;
    let mut goal_progress = Vec::new();
    let rows = stmt
        .query_map([], |r| {
            let manual: i64 = r.get(2)?;
            let todo_count: i64 = r.get(3)?;
            let done_count: i64 = r.get(4)?;
            let progress = if todo_count == 0 {
                manual as f64 / 100.0
            } else {
                done_count as f64 / todo_count as f64
            };
            Ok(GoalSlice {
                goal_id: r.get(0)?,
                title: r.get(1)?,
                todo_count,
                done_count,
                progress,
            })
        })
        .map_err(|e| e.to_string())?;
    for r in rows {
        goal_progress.push(r.map_err(|e| e.to_string())?);
    }

    // 近 7 天趋势（今天往前 6 天；completed_at 按本地日期分桶；缺日补 0）
    let mut by_date = std::collections::HashMap::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT completed_at FROM todos
                 WHERE completed=1 AND completed_at IS NOT NULL AND completed_at>=?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![week_start], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        for r in rows {
            let ms: i64 = r.map_err(|e| e.to_string())?;
            let ymd = ymd_of(
                chrono::DateTime::from_timestamp_millis(ms)
                    .unwrap_or_default()
                    .with_timezone(&Local)
                    .naive_local(),
            );
            *by_date.entry(ymd).or_insert(0i64) += 1;
        }
    }
    let mut week_trend = Vec::new();
    let today_date = NaiveDate::from_ymd_opt(
        (today / 10000) as i32,
        ((today / 100) % 100) as u32,
        (today % 100) as u32,
    )
    .ok_or("今天日期解析失败")?;
    for i in (0..7).rev() {
        let d = today_date - chrono::Duration::days(i);
        let ymd = ymd_of(d.and_time(NaiveTime::MIN));
        week_trend.push(TrendPoint {
            date: ymd,
            completed: *by_date.get(&ymd).unwrap_or(&0),
        });
    }

    Ok(DashboardStats {
        today_count,
        overdue_count,
        week_completed,
        project_distribution,
        goal_progress,
        week_trend,
    })
}

#[tauri::command]
pub fn dashboard_get(db: State<'_, Db>) -> Result<DashboardStats, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    dashboard_stats(&conn)
}

// ---------- T3.6 月总览 ----------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    pub date: i64,
    pub total: i64,
    pub done: i64,
    pub colors: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthStats {
    /// 当月每天（填充整个栅格：含前后月补位日，is_current_month 区分）
    pub days: Vec<DayStat>,
    pub month_total: i64,
    pub month_done: i64,
    pub inbox_count: i64,
}

/// 月总览：某年月的日历栅格（周日开列）+ 当月摘要。
pub fn month_stats(conn: &Connection, year: i32, month: u32) -> Result<MonthStats, String> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).ok_or("非法年月")?;
    // 周日开列：第一天所在列偏移
    let lead = first.weekday().num_days_from_sunday() as i64;
    let last_day = {
        let (y2, m2) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        NaiveDate::from_ymd_opt(y2, m2, 1).unwrap().pred_opt().unwrap().day()
    };
    let total_days = last_day as i64;

    // 当月每天聚合（date, total, done）；YYYYMMDD 区间 [YYYYMM01, 下月01)
    let start = year as i64 * 10_000 + month as i64 * 100 + 1;
    let end = start + 100;
    let mut by_date: std::collections::HashMap<i64, (i64, i64, Vec<String>)> = std::collections::HashMap::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT date, completed, color FROM (
                   SELECT t.date AS date, t.completed AS completed, t.color AS color
                   FROM todos t
                   WHERE t.date >= ?1 AND t.date < ?2 AND t.date IS NOT NULL
                     AND t.id NOT IN (SELECT todo_id FROM todo_repeat_rules)
                   UNION ALL
                   SELECT inst.date AS date, inst.completed AS completed, t.color AS color
                   FROM todo_repeat_items inst JOIN todos t ON t.id = inst.todo_id
                   WHERE inst.date >= ?1 AND inst.date < ?2
                 )",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![start, end],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?)),
            )
            .map_err(|e| e.to_string())?;
        for r in rows {
            let (d, done, color) = r.map_err(|e| e.to_string())?;
            let e = by_date.entry(d).or_insert((0, 0, Vec::new()));
            e.0 += 1;
            e.1 += done;
            if let Some(c) = color {
                if !e.2.contains(&c) {
                    e.2.push(c);
                }
            }
        }
    }
    let inbox_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM todos WHERE date IS NULL", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    // 栅格：周日起始，补前后月占位（is_current_month 由前端按日号判断——这里按周栅格输出日号+日期）
    let mut days = Vec::new();
    // 补位 = lead 个上周日号（负日号代表上月）
    for off in (1..=lead).rev() {
        let d = first - chrono::Duration::days(off);
        let ymd = ymd_of(d.and_hms_opt(0, 0, 0).unwrap());
        days.push(DayStat { date: ymd, total: 0, done: 0, colors: Vec::new() });
    }
    for day in 1..=total_days {
        let d = NaiveDate::from_ymd_opt(year, month, day as u32).unwrap();
        let ymd = ymd_of(d.and_hms_opt(0, 0, 0).unwrap());
        let (total, done, colors) = by_date.get(&ymd).cloned().unwrap_or((0, 0, Vec::new()));
        days.push(DayStat { date: ymd, total, done, colors });
    }
    // 尾部补位到下周六
    while days.len() % 7 != 0 {
        let d = first + chrono::Duration::days(days.len() as i64 - lead);
        let ymd = ymd_of(d.and_hms_opt(0, 0, 0).unwrap());
        days.push(DayStat { date: ymd, total: 0, done: 0, colors: Vec::new() });
    }

    let month_total: i64 = by_date.values().map(|(t, _, _)| t).sum();
    let month_done: i64 = by_date.values().map(|(_, d, _)| d).sum();
    Ok(MonthStats { days, month_total, month_done, inbox_count })
}

/// 批量完成某日全部未完成（单事务，50 条不卡；T3.6 验收）。
pub fn batch_complete_date(conn: &Connection, date: i64) -> Result<i64, String> {
    let now = chrono::Local::now().timestamp_millis();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    // 该日有分配人的待办：先把全员分配标记置完成（对齐 person 协作语义）
    tx.execute(
        "UPDATE todo_assignees SET completed=1, completed_at=COALESCE(completed_at,?2)
         WHERE completed=0 AND todo_id IN (SELECT id FROM todos WHERE date=?1 AND completed=0)",
        params![date, now],
    )
    .map_err(|e| e.to_string())?;
    // 置完成
    let n = tx
        .execute(
            "UPDATE todos SET completed=1, completed_at=COALESCE(completed_at,?2), updated_at=?2
             WHERE date=?1 AND completed=0",
            params![date, now],
        )
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(n as i64)
}

/// 批量取消完成某日全部已完成（batch_complete_date 的逆操作）：单事务。
/// 分配标记先行清零，再回退整体状态；不涉及 title/content，无需挂 FTS。
pub fn batch_uncomplete_date(conn: &Connection, date: i64) -> Result<i64, String> {
    let now = chrono::Local::now().timestamp_millis();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    // 该日已完成的待办：先清全员分配标记
    tx.execute(
        "UPDATE todo_assignees SET completed=0, completed_at=NULL
         WHERE todo_id IN (SELECT id FROM todos WHERE date=?1 AND completed=1)",
        params![date],
    )
    .map_err(|e| e.to_string())?;
    // 回退整体状态
    let n = tx
        .execute(
            "UPDATE todos SET completed=0, completed_at=NULL, updated_at=?2 WHERE date=?1 AND completed=1",
            params![date, now],
        )
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(n as i64)
}

#[tauri::command]
pub fn overview_month(db: State<'_, Db>, year: i32, month: u32) -> Result<MonthStats, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    month_stats(&conn, year, month)
}

#[tauri::command]
pub fn overview_batch_complete(db: State<'_, Db>, date: i64) -> Result<i64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    batch_complete_date(&conn, date)
}

/// 批量取消完成某日全部已完成（月总览）。
#[tauri::command]
pub fn overview_batch_uncomplete(db: State<'_, Db>, date: i64) -> Result<i64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    batch_uncomplete_date(&conn, date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::goal::{create_goal, set_goal_links, update_goal};
    use crate::commands::project::create_project;
    use crate::commands::todo::{create_todo, get_todo, set_completed};
    use crate::models::UpdateGoal;
    use crate::models::{CreateGoal, CreateProject, CreateTodo, GoalLinkInput, ItemType};

    /// T3.6：月历栅格 + 摘要 + 批量完成一个事务
    #[test]
    fn month_stats_and_batch() {
        let (conn, root) = temp_db("month");
        let conn = &conn;
        // 2026-09：9/1 周二；首行补位=周二偏移 lead=2（周日+周一补位）
        let mk = |title: &str, date: Option<i64>, done: bool, color: Option<&str>| {
            let input = CreateTodo {
                title: title.into(),
                content: None,
                date,
                color: color.map(String::from),
                category: None,
                priority: None,
                project_id: None,
                time: None,
                parent_id: None,
                sort_order: None,
            };
            let t = create_todo(conn, input).unwrap();
            if done {
                set_completed(conn, &t.id, true).unwrap();
            }
            t
        };
        // 9/1 两条：1 完成(blue) 1 未完成(red)
        mk("任务9-1a", Some(20260901), true, Some("blue"));
        mk("任务9-1b", Some(20260901), false, Some("red"));
        // 9/15 一条未完成（无颜色）
        mk("任务9-15", Some(20260915), false, None);
        // 收集箱一条
        mk("收集箱项", None, false, None);

        let s = month_stats(conn, 2026, 9).unwrap();
        let first_day = s.days.iter().find(|d| d.date == 20260901).unwrap();
        assert_eq!(first_day.total, 2);
        assert_eq!(first_day.done, 1);
        assert_eq!(first_day.colors, vec!["blue", "red"]);
        assert_eq!(s.month_total, 3);
        assert_eq!(s.month_done, 1);
        assert_eq!(s.inbox_count, 1);
        // 栅格长度：9月30天 + lead2(前月) + 尾补到整周 → 35 或 42（30+2=32 → 尾补5 → 35）
        assert_eq!(s.days.len() % 7, 0, "栅格整周");
        // 首格应为上月（2026-08-30 周日）占位 total=0
        assert_eq!(s.days[0].date, 20260830);

        // 批量完成 9/15 → 1 条，单事务
        let n = batch_complete_date(conn, 20260915).unwrap();
        assert_eq!(n, 1);
        let after = month_stats(conn, 2026, 9).unwrap();
        assert_eq!(after.month_done, 2);
        let _ = std::fs::remove_dir_all(root);
    }

    fn temp_db(tag: &str) -> (Connection, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-dash-test-{}-{}-{}",
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

    /// TASKS 验收：空库不报错（引导态）
    #[test]
    fn empty_db_returns_zeroes() {
        let (conn, root) = temp_db("empty");
        let conn = &conn;
        let s = dashboard_stats(conn).unwrap();
        assert_eq!(
            (s.today_count, s.overdue_count, s.week_completed),
            (0, 0, 0)
        );
        assert!(s.project_distribution.is_empty());
        assert!(s.goal_progress.is_empty());
        assert_eq!(s.week_trend.len(), 7, "趋势固定 7 天（补 0）");
        assert!(s.week_trend.iter().all(|p| p.completed == 0));
        let _ = std::fs::remove_dir_all(root);
    }

    /// TASKS 验收：10 条跨项目/日期/状态待办，数字与手工核对一致
    #[test]
    fn ten_todos_across_projects_dates_match_manual() {
        let (conn, root) = temp_db("ten");
        let conn = &conn;
        let today = local_today_ymd();
        let (y, m, d) = ((today / 10000) as i32, ((today / 100) % 100) as u32, (today % 100) as u32);
        let today_date = NaiveDate::from_ymd_opt(y, m, d).unwrap();
        let ymd_of_date = |dt: NaiveDate| -> i64 {
            dt.format("%Y%m%d").to_string().parse().unwrap()
        };
        let yesterday = ymd_of_date(today_date - chrono::Duration::days(1));

        let p1 = create_project(conn, CreateProject { name: "P1".into(), ..Default::default() }).unwrap();
        let p2 = create_project(conn, CreateProject { name: "P2".into(), ..Default::default() }).unwrap();
        let g = create_goal(conn, CreateGoal { title: "目标".into(), ..Default::default() }).unwrap();

        // 10 条：
        // 1-2 今天未完成（今日=2）
        // 3 昨天（逾期=1）
        // 4 今天完成（本周完成=1，今日不计——已完成）
        // 5-6 挂 P1（1 未完 1 完成）
        // 7-8 挂 P2（2 未完）
        // 9-10 无项目未完成
        // 目标挂待办 1、4：todo_count=2，done=1 → 50%
        let mut ids = Vec::new();
        for (i, (title, date)) in [
            ("t1", Some(today)),
            ("t2", Some(today)),
            ("t3", Some(yesterday)),
            ("t4", Some(today)),
            ("t5", Some(today)),
            ("t6", None),
            ("t7", Some(today)),
            ("t8", None),
            ("t9", None),
            ("t10", None),
        ]
        .iter()
        .enumerate()
        {
            let mut input = CreateTodo {
                title: format!("{title}-{i}"),
                content: None,
                date: *date,
                color: None,
                category: None,
                priority: None,
                project_id: None,
                time: None,
                parent_id: None,
                sort_order: None,
            };
            match i {
                3 => input.project_id = Some(p1.id.clone()), // t4 完成挂 P1
                4 => input.project_id = Some(p1.id.clone()), // t5 完成挂 P1
                6 => input.project_id = Some(p2.id.clone()), // t7 未完挂 P2
                7 => input.project_id = Some(p2.id.clone()), // t8 未完挂 P2
                _ => {}
            }
            ids.push(create_todo(conn, input).unwrap());
        }
        set_completed(conn, &ids[3].id, true).unwrap(); // t4 完成
        set_completed(conn, &ids[4].id, true).unwrap(); // t5 完成（P1 内完成 1）

        // 目标挂 t1、t4
        set_goal_links(
            conn,
            &g.id,
            &[
                GoalLinkInput { item_type: ItemType::Todo, item_id: ids[0].id.clone() },
                GoalLinkInput { item_type: ItemType::Todo, item_id: ids[3].id.clone() },
            ],
        )
        .unwrap();

        // 目标进度：主进度=关联待办完成率（F32）；故意把手动进度设为 80 以验证其被完成率覆盖
        update_goal(conn, &g.id, UpdateGoal { progress: Some(80), ..Default::default() }).unwrap();
        let s = dashboard_stats(conn).unwrap();
        // 今日未完成：t1,t2,t7（t4/t5 已完成；t3 昨天；t6/t8/t9/t10 无日期）
        assert_eq!(s.today_count, 3, "今日未完成应=3，实际 {}", s.today_count);
        assert_eq!(s.overdue_count, 1, "逾期应=1（昨天 t3）");
        assert_eq!(s.week_completed, 2, "本周完成应=2（t4,t5）");

        // 项目分布：P1 total=2 done=2（t4,t5 都完成）；P2 total=2 done=0
        let p1s = s.project_distribution.iter().find(|x| x.name == "P1").unwrap();
        assert_eq!((p1s.total, p1s.done), (2, 2));
        let p2s = s.project_distribution.iter().find(|x| x.name == "P2").unwrap();
        assert_eq!((p2s.total, p2s.done), (2, 0));

        // 目标进度：主进度=关联待办完成率 1/2=0.5（覆盖手动 80）
        let gs = s.goal_progress.iter().find(|x| x.goal_id == g.id).unwrap();
        assert_eq!((gs.todo_count, gs.done_count), (2, 1));
        assert!(
            (gs.progress - 0.5).abs() < 1e-9,
            "应为关联完成率 50%（覆盖手动 80），实际 {}",
            gs.progress
        );

        // 趋势：7 天、今天桶=2（t4,t5 的 completed_at 都是今天）
        assert_eq!(s.week_trend.len(), 7);
        assert_eq!(s.week_trend.last().unwrap().completed, 2, "今天桶应=2");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 批量取消完成：batch_complete_date 的逆操作——分配标记先行清零、整体回退、他日不动
    #[test]
    fn batch_uncomplete_reverts_batch_complete() {
        let (conn, root) = temp_db("unbatch");
        let conn = &conn;
        let mk = |title: &str, date: Option<i64>| {
            create_todo(
                conn,
                CreateTodo {
                    title: title.into(),
                    content: None,
                    date,
                    color: None,
                    category: None,
                    priority: None,
                    project_id: None,
                    time: None,
                    parent_id: None,
                    sort_order: None,
                },
            )
            .unwrap()
        };
        let a = mk("A", Some(20260915));
        let b = mk("B", Some(20260915));
        let other = mk("C", Some(20260916));

        // A 带分配人：批量完成后分配标记应被一并置位
        conn.execute(
            "INSERT INTO people (id,name,note,sort_order,archived,created_at,updated_at)
             VALUES ('p1','张三',NULL,0,0,1,1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO todo_assignees (todo_id,person_id,completed,completed_at) VALUES (?1,'p1',0,NULL)",
            params![a.id],
        )
        .unwrap();

        let n = batch_complete_date(conn, 20260915).unwrap();
        assert_eq!(n, 2);
        let (tc, ta): (i64, i64) = conn
            .query_row(
                "SELECT t.completed, ta.completed FROM todos t JOIN todo_assignees ta ON ta.todo_id=t.id WHERE t.id=?1",
                params![a.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((tc, ta), (1, 1));

        // 批量取消 → 当日已完成全回未完成（分配标记与 completed_at 清空），他日不动
        let m = batch_uncomplete_date(conn, 20260915).unwrap();
        assert_eq!(m, 2);
        let (tc, ta, ta_at): (i64, i64, Option<i64>) = conn
            .query_row(
                "SELECT t.completed, ta.completed, ta.completed_at
                 FROM todos t JOIN todo_assignees ta ON ta.todo_id=t.id WHERE t.id=?1",
                params![a.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((tc, ta), (0, 0));
        assert_eq!(ta_at, None);
        for t in [&a, &b] {
            let done = get_todo(conn, &t.id).unwrap().unwrap();
            assert!(!done.completed);
            assert!(done.completed_at.is_none());
        }
        assert!(!get_todo(conn, &other.id).unwrap().unwrap().completed, "他日不受影响");

        // 已全部未完成后再执行 → 0 条
        assert_eq!(batch_uncomplete_date(conn, 20260915).unwrap(), 0);
        let _ = std::fs::remove_dir_all(root);
    }
}
