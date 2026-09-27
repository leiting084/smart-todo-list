//! 最近操作只读日志（#32）：记录用户经 UI 的主要增删改，供「最近操作」视图回看。
//!
//! 设计取舍：
//! - **只记不撤**——撤销/回滚是另一套快照机制，风险高、收益低，本版不做；
//! - 只在**命令薄封装层**（用户动作）调用 [`record`]，不在 repo / importer / repeat 内部调用，
//!   避免批量导入、重复实例生成等内部写污染成海量噪声；
//! - 记日志一律 best-effort：`record` 失败只被调用方 `let _ =` 吞掉，绝不因日志坏了连累主操作。

use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

use crate::db::Db;

/// 保留最近多少条；超出在每次写入后按自增 id 裁剪（本地单用户，量小，够用）。
const MAX_ROWS: i64 = 500;

/// 一条操作日志（返回给前端的视图模型）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpLogEntry {
    pub id: i64,
    /// 毫秒时间戳
    pub ts: i64,
    /// create/update/delete/complete/uncomplete/archive/unarchive/import
    pub action: String,
    /// todo/project/memo/note/goal/person
    pub entity_type: String,
    /// 目标 id；批量操作为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    /// 人类可读摘要（标题/名称/批量计数）
    pub summary: String,
}

/// 追加一条操作日志（best-effort），并裁剪到最近 [`MAX_ROWS`] 条。
pub fn record(
    conn: &Connection,
    action: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    summary: &str,
) -> Result<(), String> {
    let ts = chrono::Local::now().timestamp_millis();
    conn.execute(
        "INSERT INTO op_log (ts, action, entity_type, entity_id, summary) VALUES (?1,?2,?3,?4,?5)",
        params![ts, action, entity_type, entity_id, summary],
    )
    .map_err(|e| e.to_string())?;
    // 裁剪：只保留最近 MAX_ROWS 条。id 单调自增，MAX(id)-MAX_ROWS 以下的全删。
    conn.execute(
        "DELETE FROM op_log WHERE id <= (SELECT COALESCE(MAX(id), 0) - ?1 FROM op_log)",
        params![MAX_ROWS],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 读取最近 limit 条（默认 100，钳制到 1..=MAX_ROWS），新→旧。
pub fn list(conn: &Connection, limit: Option<i64>) -> Result<Vec<OpLogEntry>, String> {
    let limit = limit.unwrap_or(100).clamp(1, MAX_ROWS);
    let mut stmt = conn
        .prepare(
            "SELECT id, ts, action, entity_type, entity_id, summary \
             FROM op_log ORDER BY id DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(OpLogEntry {
                id: r.get(0)?,
                ts: r.get(1)?,
                action: r.get(2)?,
                entity_type: r.get(3)?,
                entity_id: r.get(4)?,
                summary: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 前端拉取最近操作列表。
#[tauri::command]
pub fn oplog_list(db: State<'_, Db>, limit: Option<i64>) -> Result<Vec<OpLogEntry>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list(&conn, limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_at;
    use std::path::PathBuf;

    fn temp_db(tag: &str) -> (Connection, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-oplog-test-{}-{}-{}",
            std::process::id(),
            std::line!(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&root);
        let bkp = root.join("backups");
        std::fs::create_dir_all(&bkp).unwrap();
        let conn = open_at(&root.join("todolist.db"), &bkp).unwrap();
        (conn, root)
    }

    #[test]
    fn record_then_list_newest_first_with_limit() {
        let (conn, root) = temp_db("list");
        record(&conn, "create", "todo", Some("t1"), "写周报").unwrap();
        record(&conn, "update", "todo", Some("t1"), "写周报").unwrap();
        record(&conn, "delete", "project", None, "批量删除 3 条待办").unwrap();

        let all = list(&conn, None).unwrap();
        assert_eq!(all.len(), 3);
        // 新→旧：最后写入的 delete 在最前
        assert_eq!(all[0].action, "delete");
        assert_eq!(all[0].entity_type, "project");
        assert_eq!(all[0].entity_id, None, "批量操作 entity_id 为 NULL");
        assert_eq!(all[2].action, "create");
        assert_eq!(all[2].summary, "写周报");

        // limit 生效
        let two = list(&conn, Some(2)).unwrap();
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].action, "delete");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prunes_to_max_rows_keeping_newest() {
        let (conn, root) = temp_db("prune");
        let total = MAX_ROWS + 50;
        for i in 0..total {
            record(&conn, "update", "todo", Some("x"), &format!("条目 {i}")).unwrap();
        }
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM op_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, MAX_ROWS, "裁剪后只保留最近 {MAX_ROWS} 条");
        // 保留的应是最新的：最后写入的 summary 在最前，最早的第 0 条已被裁掉
        let newest = list(&conn, Some(1)).unwrap();
        assert_eq!(newest[0].summary, format!("条目 {}", total - 1));
        let oldest_kept: i64 = conn
            .query_row("SELECT MIN(id) FROM op_log", [], |r| r.get(0))
            .unwrap();
        assert!(oldest_kept > 1, "最早的若干条应已被裁剪（min id > 1）");
        let _ = std::fs::remove_dir_all(root);
    }
}
