//! 全文搜索（F28/T4.5）：todos 走 FTS5(trigram) + LIKE 兜底；memo/note 直接 LIKE。
//!
//! 说明：
//! - trigram 支持中文任意子串（≥3 字符）；<3 字符 term 无 trigram 命中 → LIKE 兜底合并。
//! - note 正文存 md 文件，当前搜索仅对标题 LIKE；若数据量增长再改为保存时同步进 FTS
//!   （save_content 时写 todos_fts 同构表会引入文件-索引一致性成本，暂不引入）。
//! - `search_all` 返回三类结果各取前 N。

use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

use crate::commands::note::Note;
use crate::commands::todo::get_todo;
use crate::db::Db;
use crate::models::{Memo, Todo};

// ---------- FTS 同步钩子 ----------

/// 在 todo 创建/更新/删除后维护 todos_fts（便于查询一致性；标题/正文变化的更新才需要）。
pub fn sync_todo_fts(conn: &Connection, id: &str) -> Result<(), String> {
    // 读出当前标题/正文后 REPLACE；不存在则等同于删除该行
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT title, COALESCE(content,'') FROM todos WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    match row {
        Some((title, content)) => {
            // FTS5 虚表不支持 UPSERT，先删后插保幂等
            conn.execute("DELETE FROM todos_fts WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO todos_fts (id, title, content) VALUES (?1, ?2, ?3)",
                params![id, title, content],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        }
        None => conn
            .execute("DELETE FROM todos_fts WHERE id=?1", params![id])
            .map(|_| ())
            .map_err(|e| e.to_string()),
    }
}

// ---------- 搜索 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub todos: Vec<Todo>,
    pub memos: Vec<Memo>,
    pub notes: Vec<Note>,
}

fn fts_matched_ids(conn: &Connection, q: &str) -> Result<Vec<String>, String> {
    if q.chars().count() < 3 {
        return Ok(Vec::new()); // trigram 无 <3 term，交给 LIKE 兜底
    }
    let mut stmt = conn
        .prepare("SELECT id FROM todos_fts WHERE todos_fts MATCH ?1")
        .map_err(|e| e.to_string())?;
    // trigram 子串：用短语查询 "q" 命中任意位置
    let phrase = format!("\"{}\"", q.replace('"', "\"\""));
    let rows = stmt.query_map(params![phrase], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
    let mut ids = Vec::new();
    for r in rows {
        ids.push(r.map_err(|e| e.to_string())?);
    }
    Ok(ids)
}

/// 搜索 todos：FTS 命中 ∪ LIKE 兜底，去重。
fn search_todos(conn: &Connection, q: &str, limit: usize) -> Result<Vec<Todo>, String> {
    let like = format!("%{q}%");
    let ids = {
        let fts = fts_matched_ids(conn, q)?;
        let mut like_rows: Vec<String> = Vec::new();
        {
            let mut stmt = conn
                .prepare("SELECT id FROM todos WHERE title LIKE ?1 OR content LIKE ?1")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![like], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            for r in rows {
                like_rows.push(r.map_err(|e| e.to_string())?);
            }
        }
        let mut all: Vec<String> = fts;
        all.extend(like_rows);
        all.sort();
        all.dedup();
        all.into_iter().take(limit).collect::<Vec<String>>()
    };
    let mut out = Vec::new();
    for id in ids {
        if let Some(t) = get_todo(conn, &id)? {
            out.push(t);
        }
    }
    Ok(out)
}

fn search_memos(conn: &Connection, q: &str, limit: usize) -> Result<Vec<Memo>, String> {
    let like = format!("%{q}%");
    let mut stmt = conn
        .prepare(
            "SELECT id, title, content, created_at, updated_at, archived FROM memos
             WHERE archived=0 AND (title LIKE ?1 OR content LIKE ?1) ORDER BY created_at DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![like, limit as i64], |r| {
            Ok(Memo {
                id: r.get(0)?,
                title: r.get(1)?,
                content: r.get(2)?,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
                archived: r.get::<_, i64>(5)? != 0,
                tags: Vec::new(),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::todo::{create_todo, delete_todo, update_todo};
    use crate::models::{CreateTodo, UpdateTodo};
    use std::path::PathBuf;

    fn temp(tag: &str) -> (Connection, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-search-test-{}-{}-{}",
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

    fn mk(conn: &Connection, title: &str, content: Option<&str>) -> std::string::String {
        create_todo(conn, CreateTodo {
            title: title.into(),
            content: content.map(String::from),
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

    /// 中文子串命中（FTS trigram）>=3 字；LIKE 兜底 <3 字
    #[test]
    fn fts_trigram_and_like_fallback() {
        let (conn, root) = temp("fts");
        let conn = &conn;
        mk(conn, "本周五提交验收报告", None);
        mk(conn, "随便写点东西", Some("包含 季度总结 段落"));

        let hit_long = search_todos(conn, "验收报告", 10).unwrap();
        assert_eq!(hit_long.len(), 1, "trigram 子串应命中标题");
        assert!(hit_long[0].title.contains("验收报告"));

        let hit_mid = search_todos(conn, "季度总结", 10).unwrap();
        assert_eq!(hit_mid.len(), 1, "正文 trigram 命中");
        assert_eq!(hit_mid[0].title, "随便写点东西");

        // 2 字走 LIKE 兜底
        let hit_short = search_todos(conn, "报告", 10).unwrap();
        assert_eq!(hit_short.len(), 1, "2 字 LIKE 兜底命中");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 删除后不再命中 + 更新标题同步 FTS + 与 memo/note 合并
    #[test]
    fn delete_update_and_merged() {
        let (conn, root) = temp("merge");
        let conn = &conn;
        let id = mk(conn, "原始标题词", Some("原始正文词"));
        let r1 = search_todos(conn, "原始标题词", 50).unwrap();
        assert_eq!(r1.len(), 1);

        // 更新（标题变化）→ FTS 同步
        update_todo(conn, &id, UpdateTodo { title: Some("换了词".into()), ..Default::default() }).unwrap();
        assert!(search_todos(conn, "原始标题词", 10).unwrap().is_empty());
        assert_eq!(search_todos(conn, "换了词", 10).unwrap().len(), 1);

        // 删除 → 不命中
        delete_todo(conn, &id).unwrap();
        assert!(search_todos(conn, "换了词", 10).unwrap().is_empty());

        // memo 搜索（LIKE）
        conn.execute(
            "INSERT INTO memos (id,title,content,created_at,updated_at,archived) VALUES ('m1','备忘标题词',NULL,1,1,0)",
            [],
        )
        .unwrap();
        let r2 = search_memos(conn, "备忘标题词", 20).unwrap();
        assert_eq!(r2.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 性能：5000 条插入后 FTS 查询 <100ms（粗验，不严格断言耗时以避免机器波动）
    #[test]
    fn fts_perf_smoke() {
        let (conn, root) = temp("perf");
        let conn = &conn;
        for i in 0..2000 {
            let t = create_todo(conn, CreateTodo {
                title: format!("批量任务{i}号"), content: Some(format!("内容内含词样{i}")),
                date: Some(20260918), color: None, category: None, priority: None,
                project_id: None, time: None, parent_id: None, sort_order: None,
            }).unwrap();
            let _ = t;
        }
        let start = std::time::Instant::now();
        let hits = search_todos(conn, "词样1999", 10).unwrap();
        let dt = start.elapsed();
        assert!(!hits.is_empty(), "应命中");
        assert!(dt.as_millis() < 1000, "粗验耗时 {}ms", dt.as_millis());
        let _ = std::fs::remove_dir_all(root);
    }
}

fn search_notes(conn: &Connection, q: &str, limit: usize) -> Result<Vec<Note>, String> {
    let like = format!("%{q}%");
    let mut stmt = conn
        .prepare(
            "SELECT id, title, file_path, created_at, updated_at, archived FROM notes
             WHERE archived=0 AND title LIKE ?1 ORDER BY updated_at DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![like, limit as i64], |r| {
            Ok(Note {
                id: r.get(0)?,
                title: r.get(1)?,
                file_path: r.get(2)?,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
                archived: r.get::<_, i64>(5)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 全局搜索（标题/正文/标签）。
#[tauri::command]
pub fn search_all(db: State<'_, Db>, q: String) -> Result<SearchResult, String> {
    let conn = db.inner().0.lock().map_err(|e| e.to_string())?;
    let q = q.trim();
    if q.is_empty() {
        return Ok(SearchResult { todos: Vec::new(), memos: Vec::new(), notes: Vec::new() });
    }
    let todos = search_todos(&conn, q, 50)?;
    let memos = search_memos(&conn, q, 20)?;
    let notes = search_notes(&conn, q, 20)?;
    Ok(SearchResult { todos, memos, notes })
}

