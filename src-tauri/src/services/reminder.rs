//! 提醒调度（F14/T3.4，SPEC §13.1）：后台线程每 20 秒扫描，到点弹系统通知。
//!
//! 规则：
//! - 扫描 `date=今天 AND completed=0 AND time(HH:MM) 到期` 的待办；
//! - **去重**：`notification_log` 表 PK(todo_id, date)，INSERT OR IGNORE affected==1 才发
//!   （External 踩过"疯狂弹通知"）；同一待办同一天最多一条；
//! - 全局开关：settings `reminders_enabled`（缺省开启）；
//! - 进程未运行期间到期**不补弹**（v1 明文行为）；
//! - 时钟：直接比较本地 HH:MM 字符串，不用时间戳差值（跨午夜安全）。

use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Local;
use rusqlite::{params, Connection};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::id::ymd_of;
use crate::paths;

/// 全局开关（线程内每次扫描读库判断，此开关供命令立即生效标记；简化：直接读库即可，无缓存）。
pub static REMINDER_THREAD_STARTED: AtomicBool = AtomicBool::new(false);

/// 到期待提醒的待办（today / since_hh_mm / now_hh_mm 参数化便于单测）。
///
/// §13.1：只弹 `time ∈ (上次扫描, 现在]` 的待办，**不补弹**进程未运行期间已错过的
/// 提醒。`since_hh_mm` 由调用方（后台线程）传入上一轮扫描的 HH:MM；首轮等于当前
/// 时刻，故窗口为空、启动瞬间不刷屏。用 HH:MM 字符串比较，跨午夜安全。
pub fn due_todos(
    conn: &Connection,
    today: i64,
    since_hh_mm: &str,
    now_hh_mm: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title FROM (
               SELECT t.id AS id, t.title AS title, t.time AS time
               FROM todos t
               WHERE t.date=?1 AND t.completed=0 AND t.time IS NOT NULL AND t.time != ''
                 AND t.time > ?2 AND t.time <= ?3
                 AND t.id NOT IN (SELECT todo_id FROM todo_repeat_rules)
               UNION ALL
               SELECT inst.id AS id, t.title AS title, t.time AS time
               FROM todo_repeat_items inst JOIN todos t ON t.id = inst.todo_id
               WHERE inst.date=?1 AND inst.completed=0 AND t.time IS NOT NULL AND t.time != ''
                 AND t.time > ?2 AND t.time <= ?3
             ) ORDER BY time",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![today, since_hh_mm, now_hh_mm], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 尝试占用提醒名额：首次（affected==1）→ 可发；重复 → 已发过。
pub fn try_mark_fired(conn: &Connection, todo_id: &str, today: i64) -> Result<bool, String> {
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO notification_log (todo_id, date, fired_at) VALUES (?1, ?2, ?3)",
            params![todo_id, today, Local::now().timestamp_millis()],
        )
        .map_err(|e| e.to_string())?;
    Ok(n == 1)
}

fn reminders_enabled(conn: &Connection) -> bool {
    conn.query_row("SELECT value FROM settings WHERE key='reminders_enabled'", [], |r| {
        r.get::<_, Option<String>>(0)
    })
    .map(|v| v.as_deref() != Some("false"))
    .unwrap_or(true)
}

/// 一轮扫描（返回发出的条数；app 为 None 时仅走 DB 逻辑不发通知——单测用）。
/// `since_hh_mm` 为上一轮扫描时刻，本轮只弹 `time ∈ (since, now]`（不补弹）。
pub fn scan_once(conn: &Connection, app: Option<&AppHandle>, since_hh_mm: &str) -> Result<usize, String> {
    if !reminders_enabled(conn) {
        return Ok(0);
    }
    let today = ymd_of(Local::now().naive_local());
    let now_hh_mm = Local::now().format("%H:%M").to_string();
    let due = due_todos(conn, today, since_hh_mm, &now_hh_mm)?;
    let mut fired = 0;
    for (todo_id, title) in due {
        if !try_mark_fired(conn, &todo_id, today)? {
            continue;
        }
        if let Some(app) = app {
            let _ = app.notification().builder().title("待办提醒").body(&title).show();
            let _ = app.emit("reminder-fired", todo_id.clone()); // 主窗可定位该条
        }
        fired += 1;
    }
    Ok(fired)
}

/// 启动后台扫描线程（app 启动时调用一次）。
pub fn spawn(app: AppHandle) {
    if REMINDER_THREAD_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        // 对抗复核③修复：0 点转点守护——日期变化即生成重复实例+顺延（不依赖前端轮询）
        let mut last_day: i64 = 0;
        // §13.1 不补弹：首轮窗口下界=当前时刻，进程未运行期间错过的提醒不再刷屏
        let mut last_scan = Local::now().format("%H:%M").to_string();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(20));
            // 独立连接（WAL 多连接安全）；打开失败静默跳过本轮
            let db_path = paths::app_paths().db_path.clone();
            let Ok(conn) = Connection::open(&db_path) else { continue };
            let _ = conn.pragma_update(None, "journal_mode", "WAL");
            let _ = conn.pragma_update(None, "foreign_keys", "ON");
            let today = ymd_of(Local::now().naive_local());
            if today != last_day {
                // 首轮（last_day=0）与跨天都执行：生成今日重复实例 + 自动顺延逾期未完成
                let _ = crate::services::repeat::generate_instances(&conn, Local::now().date_naive());
                let _ = crate::services::repeat::postpone_overdue(&conn, Local::now().date_naive());
            }
            last_day = today;
            let now_scan = Local::now().format("%H:%M").to_string();
            let _ = scan_once(&conn, Some(&app), &last_scan);
            last_scan = now_scan;
        }
    });
}

#[tauri::command]
/// 查询提醒开关（设置页用）。
pub fn reminders_get_enabled(db: tauri::State<'_, crate::db::Db>) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(reminders_enabled(&conn))
}

#[tauri::command]
/// 设置提醒开关（设置页用）。
pub fn reminders_set_enabled(db: tauri::State<'_, crate::db::Db>, enabled: bool) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES ('reminders_enabled', ?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![enabled.to_string(), Local::now().timestamp_millis()],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::todo::create_todo;
    use crate::models::CreateTodo;
    use std::path::PathBuf;

    fn temp_db(tag: &str) -> (Connection, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-reminder-test-{}-{}-{}",
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

    fn todo_at(conn: &Connection, title: &str, time: &str) -> crate::models::Todo {
        create_todo(conn, CreateTodo {
            title: title.into(),
            content: None,
            date: Some(20260917),
            color: None,
            category: None,
            priority: None,
            project_id: None,
            time: Some(time.into()),
            parent_id: None,
            sort_order: None,
        })
        .unwrap()
    }

    /// TASKS 验收：time 设 1 分钟后准时弹（此处验证到期判定：<=now 即到期）
    #[test]
    fn due_filter_and_dedup() {
        let (conn, root) = temp_db("due");
        let conn = &conn;
        let t1 = todo_at(conn, "已到点", "08:00");
        let _t2 = todo_at(conn, "未到点", "23:59");
        let _t3 = todo_at(conn, "无时间", "09:00");
        conn.execute("UPDATE todos SET time=NULL WHERE title='无时间'", []).unwrap();

        // 现在 08:30、窗口从当日零点起：t1 到期，t2 未到
        let due = due_todos(conn, 20260917, "00:00", "08:30").unwrap();
        assert_eq!(due.len(), 1, "只有已到点的进来：{due:?}");
        assert_eq!(due[0].0, t1.id);

        // §13.1 不补弹：窗口下界已过 08:00 → t1 不再进 due（进程晚开不刷屏）
        assert!(
            due_todos(conn, 20260917, "08:15", "08:30")
                .unwrap()
                .iter()
                .all(|(id, _)| id != &t1.id),
            "窗口 (08:15,08:30] 不应包含 08:00 的 t1"
        );
        // 08:00 恰落在窗口 (07:59,08:00] 内 → 到期
        assert!(due_todos(conn, 20260917, "07:59", "08:00")
            .unwrap()
            .iter()
            .any(|(id, _)| id == &t1.id));

        // 去重：第一次 mark 成功、第二次跳过（疯狂弹防护）
        assert!(try_mark_fired(conn, &t1.id, 20260917).unwrap());
        assert!(!try_mark_fired(conn, &t1.id, 20260917).unwrap());

        // 不同日不受影响
        assert!(try_mark_fired(conn, &t1.id, 20260918).unwrap());

        // 完成后不再进 due
        crate::commands::todo::set_completed(conn, &t1.id, true).unwrap();
        assert_eq!(due_todos(conn, 20260917, "00:00", "08:30").unwrap().len(), 0);

        // 开关：关闭后 scan_once 不发
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES ('reminders_enabled','false',1)
             ON CONFLICT(key) DO UPDATE SET value='false'",
            [],
        )
        .unwrap();
        assert_eq!(scan_once(conn, None, "00:00").unwrap(), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 0002 迁移后的表存在性 + scan_once 在空库/无开关时不报错
    #[test]
    fn migration_tables_and_scan_safe() {
        let (conn, root) = temp_db("mig");
        let conn = &conn;
        for t in ["todo_repeat_rules", "todo_repeat_items", "notification_log"] {
            let c: i64 = conn
                .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name=?1", params![t], |r| r.get(0))
                .unwrap();
            assert_eq!(c, 1, "{t} 应存在");
        }
        let v: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations WHERE version=2", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1, "迁移版本 2 已登记");
        assert_eq!(scan_once(conn, None, "00:00").unwrap(), 0);
        let _ = std::fs::remove_dir_all(root);
    }
}
