//! 标签命令（F8）：tag_create/rename/delete、item_set_tags、tags_list、items_by_tag。
//!
//! 三级实体（todo/memo/note）通用 item_tags 关联；标签与实体解耦——
//! 删标签只清关联（ON DELETE CASCADE），不删实体；改名只动 tags.name，全局引用即时生效。

use rusqlite::{params, types::Value, Connection, OptionalExtension};
use tauri::State;

use crate::db::Db;
use crate::id::new_id;
use crate::models::{ItemType, Tag, TagCount};

fn map_tag(row: &rusqlite::Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: row.get("id")?,
        name: row.get("name")?,
    })
}

/// 按名取标签，不存在则创建；返回（id, name）。同名复用，不重复建。
pub fn ensure_tag(conn: &Connection, name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("标签名不能为空".into());
    }
    if let Some(id) = conn
        .query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| r.get::<_, String>(0))
        .optional()
        .map_err(|e| e.to_string())?
    {
        return Ok(id);
    }
    let id = new_id();
    conn.execute("INSERT INTO tags (id, name) VALUES (?1, ?2)", params![id, name])
        .map_err(|e| format!("创建标签失败：{e}"))?;
    Ok(id)
}

// ---------- repo ----------

pub fn create_tag(conn: &Connection, name: &str) -> Result<Tag, String> {
    let id = ensure_tag(conn, name)?;
    conn.query_row("SELECT id, name FROM tags WHERE id = ?1", params![id], map_tag)
        .map_err(|e| e.to_string())
}

pub fn rename_tag(conn: &Connection, id: &str, new_name: &str) -> Result<(), String> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err("标签名不能为空".into());
    }
    let n = conn
        .execute("UPDATE tags SET name = ?2 WHERE id = ?1", params![id, new_name])
        .map_err(|e| format!("重命名失败（可能与其他标签重名）：{e}"))?;
    if n == 0 {
        return Err(format!("标签不存在：{id}"));
    }
    Ok(())
}

pub fn delete_tag(conn: &Connection, id: &str) -> Result<(), String> {
    let n = conn
        .execute("DELETE FROM tags WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("标签不存在：{id}"));
    }
    // item_tags 由 ON DELETE CASCADE 自动清理；实体不受影响（FK=ON）。
    Ok(())
}

/// 整体替换某实体的标签集合（单事务）。names 去重去空，缺失的标签自动创建。
pub fn set_item_tags(
    conn: &Connection,
    item_type: ItemType,
    item_id: &str,
    names: &[String],
) -> Result<(), String> {
    let ty = item_type.as_str();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM item_tags WHERE item_type = ?1 AND item_id = ?2",
        params![ty, item_id],
    )
    .map_err(|e| e.to_string())?;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for raw in names {
        let name = raw.trim();
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        let tag_id = ensure_tag(&tx, name)?;
        tx.execute(
            "INSERT OR IGNORE INTO item_tags (tag_id, item_type, item_id) VALUES (?1, ?2, ?3)",
            params![tag_id, ty, item_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn list_tags(conn: &Connection) -> Result<Vec<TagCount>, String> {
    let sql = "SELECT t.id, t.name,
                      COUNT(it.item_id) AS total,
                      SUM(CASE WHEN it.item_type='todo' THEN 1 ELSE 0 END) AS todos,
                      SUM(CASE WHEN it.item_type='memo' THEN 1 ELSE 0 END) AS memos,
                      SUM(CASE WHEN it.item_type='note' THEN 1 ELSE 0 END) AS notes
               FROM tags t LEFT JOIN item_tags it ON it.tag_id = t.id
               GROUP BY t.id ORDER BY t.name";
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(TagCount {
                id: r.get(0)?,
                name: r.get(1)?,
                total: r.get::<_, i64>(2)?,
                todos: r.get::<_, i64>(3)?,
                memos: r.get::<_, i64>(4)?,
                notes: r.get::<_, i64>(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 查某标签下某类型（或全部类型）的实体 id。
pub fn item_ids_by_tag(
    conn: &Connection,
    tag_name: &str,
    item_type: Option<ItemType>,
) -> Result<Vec<String>, String> {
    let mut sql = String::from(
        "SELECT it.item_id FROM item_tags it JOIN tags t ON t.id = it.tag_id WHERE t.name = ?1",
    );
    let mut vals: Vec<Value> = vec![Value::Text(tag_name.to_string())];
    if let Some(ty) = item_type {
        sql.push_str(" AND it.item_type = ?2");
        vals.push(Value::Text(ty.as_str().to_string()));
    }
    sql.push_str(" ORDER BY it.item_id");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(vals), |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

// ---------- Tauri 命令 ----------

#[tauri::command]
pub fn tag_create(db: State<'_, Db>, name: String) -> Result<Tag, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    create_tag(&conn, &name)
}

#[tauri::command]
pub fn tag_rename(db: State<'_, Db>, id: String, name: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    rename_tag(&conn, &id, &name)
}

#[tauri::command]
pub fn tag_delete(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    delete_tag(&conn, &id)
}

#[tauri::command]
pub fn item_set_tags(
    db: State<'_, Db>,
    item_type: String,
    item_id: String,
    names: Vec<String>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let ty = ItemType::parse(&item_type)?;
    set_item_tags(&conn, ty, &item_id, &names)
}

#[tauri::command]
pub fn tags_list(db: State<'_, Db>) -> Result<Vec<TagCount>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_tags(&conn)
}

#[tauri::command]
pub fn items_by_tag(
    db: State<'_, Db>,
    name: String,
    item_type: Option<String>,
) -> Result<Vec<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let ty = match item_type {
        Some(s) => Some(ItemType::parse(&s)?),
        None => None,
    };
    item_ids_by_tag(&conn, &name, ty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::todo::{create_todo, list_todos};
    use crate::models::{CreateTodo, TodoFilter};
    use std::path::PathBuf;

    fn temp_db(tag: &str) -> (Connection, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-tag-test-{}-{}-{}",
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

    fn todo(conn: &Connection, title: &str) -> crate::models::Todo {
        create_todo(conn, CreateTodo {
            title: title.into(),
            content: None,
            date: None,
            color: None,
            category: None,
            priority: None,
            project_id: None,
            time: None,
            parent_id: None,
            sort_order: None,
        })
        .unwrap()
    }

    #[test]
    fn tag_todo_delete_tag_keeps_entity() {
        let (conn, root) = temp_db("del");
        let t = todo(&conn, "带标签任务");
        set_item_tags(&conn, ItemType::Todo, &t.id, &["会议".into(), "重要".into()]).unwrap();

        // 同名标签复用，不重复
        let id1 = ensure_tag(&conn, "会议").unwrap();
        let tags = list_tags(&conn).unwrap();
        assert_eq!(tags.len(), 2, "应有 2 个不同标签");
        assert!(tags.iter().any(|x| x.name == "会议" && x.total == 1 && x.todos == 1));

        // 同一标签也可挂 memo 类型（M2 才有 UI，这里验数据模型通用）
        set_item_tags(&conn, ItemType::Memo, "memo-x", &["会议".into()]).unwrap();
        let ids = item_ids_by_tag(&conn, "会议", None).unwrap();
        assert!(ids.contains(&t.id) && ids.contains(&"memo-x".to_string()));

        // 删标签：该标签关联清空（级联），todo 实体及其"重要"标签仍在
        delete_tag(&conn, &id1).unwrap();
        assert!(item_ids_by_tag(&conn, "会议", None).unwrap().is_empty());
        assert!(get_todo_title(&conn, &t.id), "删标签不能删 todo");
        let meeting_links: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM item_tags WHERE tag_id=?1",
                params![id1],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(meeting_links, 0, "被删标签的关联应级联清空");
        assert_eq!(tags_of_todo(&conn, &t.id), vec!["重要"], "todo 的另一标签应保留");
        let _ = std::fs::remove_dir_all(root);
    }

    fn get_todo_title(conn: &Connection, id: &str) -> bool {
        conn.query_row("SELECT 1 FROM todos WHERE id=?1", params![id], |_| Ok(())).is_ok()
    }

    fn tags_of_todo(conn: &Connection, id: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT t.name FROM item_tags it JOIN tags t ON t.id=it.tag_id
                 WHERE it.item_type='todo' AND it.item_id=?1 ORDER BY t.name",
            )
            .unwrap();
        stmt.query_map(params![id], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn rename_is_global_and_instant() {
        let (conn, root) = temp_db("rename");
        let t = todo(&conn, "任务");
        set_item_tags(&conn, ItemType::Todo, &t.id, &["旧名".into()]).unwrap();
        let tag = list_tags(&conn).unwrap().into_iter().find(|x| x.name == "旧名").unwrap();
        rename_tag(&conn, &tag.id, "新名").unwrap();
        // 关联按 tag_id，改名后用新名即可查到实体
        let ids = item_ids_by_tag(&conn, "新名", Some(ItemType::Todo)).unwrap();
        assert_eq!(ids, vec![t.id]);
        assert!(item_ids_by_tag(&conn, "旧名", None).unwrap().is_empty());
        // 重名冲突报错
        set_item_tags(&conn, ItemType::Todo, "x", &["另一个".into()]).unwrap();
        let other = list_tags(&conn).unwrap().into_iter().find(|x| x.name == "另一个").unwrap();
        assert!(rename_tag(&conn, &other.id, "新名").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn set_tags_replaces_and_dedupes() {
        let (conn, root) = temp_db("replace");
        let t = todo(&conn, "任务");
        set_item_tags(&conn, ItemType::Todo, &t.id, &["A".into(), "B".into()]).unwrap();
        assert_eq!(tags_of(&conn, &t.id), vec!["A", "B"]);
        // 整体替换：去掉 A/B，加 C（大小写/空白去重）
        set_item_tags(
            &conn,
            ItemType::Todo,
            &t.id,
            &[" C ".into(), "c".into(), "".into()],
        )
        .unwrap();
        assert_eq!(tags_of(&conn, &t.id), vec!["C"], "去空白+忽略大小写去重");

        // 空标签 count=0（前端侧栏据此隐藏）
        create_tag(&conn, "悬空标签").unwrap();
        let counts = list_tags(&conn).unwrap();
        let dangling = counts.iter().find(|x| x.name == "悬空标签").unwrap();
        assert_eq!(dangling.total, 0);
        let _ = std::fs::remove_dir_all(root);
    }

    fn tags_of(conn: &Connection, todo_id: &str) -> Vec<String> {
        let todos = list_todos(
            conn,
            &TodoFilter {
                ..Default::default()
            },
        )
        .unwrap();
        todos
            .into_iter()
            .find(|t| t.id == todo_id)
            .map(|t| t.tags)
            .unwrap_or_default()
    }
}
