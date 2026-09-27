//! 双向链接命令（F7/T2.1+）：实体关联链接查询。
//! T2.1 用于"备忘详情可见来源 todo"；T2.4 扩展 wiki-link diff 同步与 LinkPanel 全量。

use rusqlite::{params, Connection};
use tauri::State;

use crate::db::Db;
use crate::models::{ItemType, Link};

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Link> {
    let parse_type = |s: String| ItemType::parse(&s).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()));
    let from_type = row.get::<_, String>("from_type").and_then(parse_type)?;
    let to_type = row.get::<_, String>("to_type").and_then(parse_type)?;
    Ok(Link {
        id: row.get("id")?,
        from_type,
        from_id: row.get("from_id")?,
        to_type,
        to_id: row.get("to_id")?,
        relation: row.get("relation")?,
        created_at: row.get("created_at")?,
    })
}

/// 某实体的双向链接（作为 from 或 as to；T2.4 LinkPanel 复用）。
pub fn links_for(conn: &Connection, item_type: ItemType, item_id: &str) -> Result<Vec<Link>, String> {
    let ty = item_type.as_str();
    let mut stmt = conn
        .prepare(
            "SELECT id, from_type, from_id, to_type, to_id, relation, created_at
             FROM links WHERE (from_type=?1 AND from_id=?2) OR (to_type=?1 AND to_id=?2)
             ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![ty, item_id], map_row)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn links_for_entity(
    db: State<'_, Db>,
    item_type: String,
    item_id: String,
) -> Result<Vec<Link>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let ty = ItemType::parse(&item_type)?;
    links_for(&conn, ty, &item_id)
}
