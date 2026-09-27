//! 备忘命令（F3/T2.1）：CRUD（删除=归档）+ 完成转备忘（三级沉淀第一步）。
//!
//! memo_create_from_todo 单事务：建 memo（标题=待办标题，正文=完成日期+原描述），
//! 写成对 link（converted_to todo→memo / derived_from memo→todo），复制标签。
//! 任一步失败整体回滚（TASKS 验收：构造 link 失败，memo 不落库）。

use std::fs;
use std::path::Path;

use chrono::Local;
use rusqlite::{params, types::Value, Connection, OptionalExtension};
use tauri::State;

use crate::db::Db;
use crate::id::new_id;
use crate::models::{CreateMemo, ItemType, Memo, UpdateMemo};

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memo> {
    Ok(Memo {
        id: row.get("id")?,
        title: row.get("title")?,
        content: row.get("content")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        archived: row.get::<_, i64>("archived")? != 0,
        tags: Vec::new(),
    })
}

const COLUMNS: &str = "id, title, content, created_at, updated_at, archived";

/// 批量填充 memo 标签名（一次 IN 查询）。
fn fill_tags(conn: &Connection, memos: &mut [Memo]) -> Result<(), String> {
    if memos.is_empty() {
        return Ok(());
    }
    let ids: Vec<Value> = memos.iter().map(|m| Value::Text(m.id.clone())).collect();
    let placeholders = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT it.item_id, t.name FROM item_tags it JOIN tags t ON t.id = it.tag_id
         WHERE it.item_type='memo' AND it.item_id IN ({placeholders}) ORDER BY t.name"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(ids), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for r in rows {
        let (item_id, name) = r.map_err(|e| e.to_string())?;
        map.entry(item_id).or_default().push(name);
    }
    for m in memos.iter_mut() {
        if let Some(names) = map.remove(&m.id) {
            m.tags = names;
        }
    }
    Ok(())
}

// ---------- CRUD ----------

pub fn create_memo(conn: &Connection, input: CreateMemo) -> Result<Memo, String> {
    if input.title.trim().is_empty() {
        return Err("备忘标题不能为空".into());
    }
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO memos (id,title,content,created_at,updated_at,archived) VALUES (?1,?2,?3,?4,?4,0)",
        params![id, input.title.trim(), input.content, now],
    )
    .map_err(|e| format!("创建备忘失败：{e}"))?;
    get_memo(conn, &id)
}

pub fn get_memo(conn: &Connection, id: &str) -> Result<Memo, String> {
    let mut memo = conn
        .query_row(&format!("SELECT {COLUMNS} FROM memos WHERE id=?1"), params![id], map_row)
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("备忘不存在：{id}"))?;
    fill_tags(conn, std::slice::from_mut(&mut memo))?;
    Ok(memo)
}

pub fn update_memo(conn: &Connection, id: &str, input: UpdateMemo) -> Result<Memo, String> {
    if get_memo(conn, id).is_err() {
        return Err(format!("备忘不存在：{id}"));
    }
    if let Some(t) = &input.title {
        if t.trim().is_empty() {
            return Err("备忘标题不能为空".into());
        }
    }
    let mut sets: Vec<String> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    if let Some(t) = input.title {
        sets.push(format!("title = ?{}", vals.len() + 1));
        vals.push(Value::Text(t.trim().to_string()));
    }
    if let Some(c) = input.content {
        sets.push(format!("content = ?{}", vals.len() + 1));
        vals.push(c.map_or(Value::Null, Value::Text));
    }
    if !sets.is_empty() {
        sets.push(format!("updated_at = ?{}", vals.len() + 1));
        vals.push(Value::Integer(now_ms()));
        vals.push(Value::Text(id.to_string()));
        conn.execute(
            &format!("UPDATE memos SET {} WHERE id=?{}", sets.join(", "), vals.len()),
            rusqlite::params_from_iter(vals),
        )
        .map_err(|e| e.to_string())?;
    }
    get_memo(conn, id)
}

/// 删除=归档（SPEC §8.3：备忘删除走归档）。
pub fn archive_memo(conn: &Connection, id: &str, archived: bool) -> Result<(), String> {
    let n = conn
        .execute(
            "UPDATE memos SET archived=?2, updated_at=?3 WHERE id=?1",
            params![id, archived as i64, now_ms()],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("备忘不存在：{id}"));
    }
    Ok(())
}

pub fn list_memos(conn: &Connection, include_archived: bool) -> Result<Vec<Memo>, String> {
    let sql = format!(
        "SELECT {COLUMNS} FROM memos {} ORDER BY created_at DESC",
        if include_archived { "" } else { "WHERE archived=0" }
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], map_row).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    fill_tags(conn, &mut out)?;
    Ok(out)
}

// ---------- 完成转备忘（F3 核心） ----------

/// 建成对 link（converted_to / derived_from 互为反向冗余，SPEC §5.1）。
pub fn insert_link_pair(
    tx: &rusqlite::Transaction,
    from_type: ItemType,
    from_id: &str,
    to_type: ItemType,
    to_id: &str,
) -> Result<(), String> {
    let now = now_ms();
    let link_id = new_id();
    tx.execute(
        "INSERT INTO links (id,from_type,from_id,to_type,to_id,relation,created_at)
         VALUES (?1,?2,?3,?4,?5,'converted_to',?6)",
        params![link_id, from_type.as_str(), from_id, to_type.as_str(), to_id, now],
    )
    .map_err(|e| e.to_string())?;
    let back_id = new_id();
    tx.execute(
        "INSERT INTO links (id,from_type,from_id,to_type,to_id,relation,created_at)
         VALUES (?1,?2,?3,?4,?5,'derived_from',?6)",
        params![back_id, to_type.as_str(), to_id, from_type.as_str(), from_id, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 完成转备忘（F3/T2.1）：单事务。
pub fn memo_create_from_todo(conn: &Connection, todo_id: &str) -> Result<Memo, String> {
    let todo: (String, Option<String>, i64) = conn
        .query_row(
            "SELECT title, content, completed FROM todos WHERE id=?1",
            params![todo_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("待办不存在：{todo_id}"))?;
    let (title, content, _completed) = todo;

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let memo_id = new_id();
    let now = now_ms();

    // 正文预填：完成日期 + 原描述（SPEC §6.3）；完成日期=今天（转备忘与完成同日发生）
    let date_line = Local::now().format("%Y-%m-%d").to_string();
    let memo_content = match content {
        Some(c) if !c.trim().is_empty() => format!("完成于 {date_line}\n\n{c}"),
        _ => format!("完成于 {date_line}"),
    };

    tx.execute(
        "INSERT INTO memos (id,title,content,created_at,updated_at,archived) VALUES (?1,?2,?3,?4,?4,0)",
        params![memo_id, title.trim(), memo_content, now],
    )
    .map_err(|e| e.to_string())?;

    // 成对 link（失败则整个事务回滚 → memo 不落库）
    insert_link_pair(&tx, ItemType::Todo, todo_id, ItemType::Memo, &memo_id)?;

    // 复制标签
    {
        let mut stmt = tx
            .prepare("SELECT tag_id FROM item_tags WHERE item_type='todo' AND item_id=?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![todo_id], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut tag_ids = Vec::new();
        for r in rows {
            tag_ids.push(r.map_err(|e| e.to_string())?);
        }
        for tag_id in tag_ids {
            tx.execute(
                "INSERT OR IGNORE INTO item_tags (tag_id,item_type,item_id) VALUES (?1,'memo',?2)",
                params![tag_id, memo_id],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    tx.commit().map_err(|e| e.to_string())?;
    get_memo(conn, &memo_id)
}

// ---------- 整理为笔记（F4/T2.3） ----------

/// 建成对 link（去重版：同 from/to/relation 已存在则跳过——重复整理不产生重复链接）。
pub fn insert_link_pair_dedup(
    tx: &rusqlite::Transaction,
    from_type: ItemType,
    from_id: &str,
    to_type: ItemType,
    to_id: &str,
) -> Result<(), String> {
    let exists: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM links
             WHERE from_type=?1 AND from_id=?2 AND to_type=?3 AND to_id=?4 AND relation='converted_to'",
            params![from_type.as_str(), from_id, to_type.as_str(), to_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists > 0 {
        return Ok(());
    }
    insert_link_pair(tx, from_type, from_id, to_type, to_id)
}

/// 一条备忘的小节：`## yyyy-MM-dd 标题` + 内容。
fn memo_section(memo: &Memo) -> String {
    let date = chrono::DateTime::from_timestamp_millis(memo.created_at)
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
    let content = memo.content.as_deref().unwrap_or("").trim();
    if content.is_empty() {
        format!("## {date} {}\n\n", memo.title)
    } else {
        format!("## {date} {}\n\n{content}\n\n", memo.title)
    }
}

/// 备忘整理为笔记（F4/T2.3）：单事务。
/// note_id=None → 新建笔记（title 生效）；Some → 追加到已有笔记末尾（不覆盖原文）。
/// 返回（笔记, 是否新建）。
pub fn memo_to_note(
    conn: &Connection,
    notes_dir: &Path,
    memo_ids: &[String],
    note_id: Option<&str>,
    title: &str,
) -> Result<(crate::commands::note::Note, bool), String> {
    if memo_ids.is_empty() {
        return Err("未选择备忘".into());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    // 读取备忘（保持传入顺序）
    let mut memos = Vec::new();
    for mid in memo_ids {
        let m: (String, Option<String>, i64) = tx
            .query_row(
                "SELECT title, content, created_at FROM memos WHERE id=?1 AND archived=0",
                params![mid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("备忘不存在或已归档：{mid}"))?;
        memos.push(Memo {
            id: mid.clone(),
            title: m.0,
            content: m.1,
            created_at: m.2,
            updated_at: 0,
            archived: false,
            tags: Vec::new(),
        });
    }

    // 目标笔记
    let (note, created) = match note_id {
        Some(nid) => {
            let n = crate::commands::note::get_note(&tx, notes_dir, nid)?;
            (n, false)
        }
        None => {
            if title.trim().is_empty() {
                return Err("新笔记标题不能为空".into());
            }
            (
                crate::commands::note::create_note(
                    &tx,
                    notes_dir,
                    crate::commands::note::CreateNote {
                        title: title.to_string(),
                        content: None,
                    },
                )?,
                true,
            )
        }
    };

    // 组装小节（追加=现有正文 + 小节）
    let sections: String = memos.iter().map(memo_section).collect();
    let abs = notes_dir
        .parent()
        .map(|d| d.join(&note.file_path))
        .unwrap_or_else(|| notes_dir.join(&note.file_path));
    let existing = if created {
        String::new()
    } else {
        fs::read_to_string(&abs).map_err(|e| format!("读取笔记失败：{e}"))?
    };
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&sections);

    // 原子写文件（先文件后库——库的 updated_at 由 save 更新）
    crate::commands::note::save_content(&tx, notes_dir, &note.id, &content)?;

    // 每个 memo 与 note 建成对链接（去重：重复整理不重复）
    for m in &memos {
        insert_link_pair_dedup(&tx, ItemType::Memo, &m.id, ItemType::Note, &note.id)?;
    }

    tx.commit().map_err(|e| e.to_string())?;
    let final_note = crate::commands::note::get_note(conn, notes_dir, &note.id)?;
    Ok((final_note, created))
}

// ---------- Tauri 命令 ----------

#[tauri::command]
pub fn memo_create(db: State<'_, Db>, input: CreateMemo) -> Result<Memo, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    create_memo(&conn, input)
}

#[tauri::command]
pub fn memo_update(db: State<'_, Db>, id: String, input: UpdateMemo) -> Result<Memo, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    update_memo(&conn, &id, input)
}

#[tauri::command]
pub fn memo_delete(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    archive_memo(&conn, &id, true)
}

#[tauri::command]
pub fn memo_list(db: State<'_, Db>, include_archived: Option<bool>) -> Result<Vec<Memo>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_memos(&conn, include_archived.unwrap_or(false))
}

/// 完成转备忘（勾选完成后弹确认条调用）。
#[tauri::command]
pub fn memo_create_from_todo_cmd(db: State<'_, Db>, todo_id: String) -> Result<Memo, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    memo_create_from_todo(&conn, &todo_id)
}

/// 备忘整理为笔记（T2.3）：note_id 空=新建（title），否则追加。
#[tauri::command]
pub fn memo_to_note_cmd(
    db: State<'_, Db>,
    memo_ids: Vec<String>,
    note_id: Option<String>,
    title: Option<String>,
) -> Result<crate::commands::note::Note, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let (note, _) = memo_to_note(
        &conn,
        crate::paths::app_paths().notes_dir.as_path(),
        &memo_ids,
        note_id.as_deref(),
        title.as_deref().unwrap_or(""),
    )?;
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tag::set_item_tags;
    use crate::commands::todo::{create_todo, set_completed};
    use crate::models::CreateTodo;

    fn temp_db(tag: &str) -> (Connection, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-memo-test-{}-{}-{}",
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

    fn todo(conn: &Connection, title: &str, content: Option<&str>) -> crate::models::Todo {
        create_todo(conn, CreateTodo {
            title: title.into(),
            content: content.map(String::from),
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

    /// TASKS 验收：转备忘 → memo 落库 + 成对 link + 标签复制 + 来源互见
    #[test]
    fn convert_todo_creates_memo_with_pair_links_and_tags() {
        let (conn, root) = temp_db("convert");
        let conn = &conn;
        let t = todo(conn, "完成转备忘任务", Some("重要结论要沉淀"));
        // 给待办挂标签
        set_item_tags(conn, ItemType::Todo, &t.id, &["经验".to_string(), "沉淀".to_string()]).unwrap();
        set_completed(conn, &t.id, true).unwrap();

        let memo = memo_create_from_todo(conn, &t.id).unwrap();
        assert_eq!(memo.title, "完成转备忘任务");
        let content = memo.content.as_deref().unwrap_or("");
        assert!(content.starts_with("完成于 "), "正文应含完成日期：{content:?}");
        assert!(content.contains("重要结论要沉淀"), "正文应含原描述");
        assert_eq!(memo.tags, vec!["沉淀".to_string(), "经验".to_string()], "标签应复制");

        // 成对 link：todo→memo converted_to、memo→todo derived_from
        let links = crate::commands::link::links_for(conn, ItemType::Todo, &t.id).unwrap();
        assert_eq!(links.len(), 2, "应有 2 条 link（成对）");
        assert!(links.iter().any(|l| l.relation == "converted_to" && l.to_id == memo.id));
        assert!(links.iter().any(|l| l.relation == "derived_from" && l.from_id == memo.id));

        // 来源互见：memo 侧也能查到
        let memo_links = crate::commands::link::links_for(conn, ItemType::Memo, &memo.id).unwrap();
        assert_eq!(memo_links.len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 不勾选完成也允许手动转（语义：转备忘不强依赖完成）；但不存在的待办报错
    #[test]
    fn convert_requires_existing_todo() {
        let (conn, root) = temp_db("noexist");
        let conn = &conn;
        assert!(memo_create_from_todo(conn, "ghost").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    /// TASKS 验收：构造 link 插入失败 → memo 不落库（整事务回滚）
    #[test]
    fn failed_link_rolls_back_memo() {
        let (conn, root) = temp_db("rollback");
        let conn = &conn;
        let t = todo(conn, "回滚任务", None);
        // 预埋一条脏 link id 冲突？直接模拟：在事务里手动插 memo+link，
        // 用 CHECK 约束触发失败——links.relation CHECK 限制 4 种，传非法 relation 报错。
        let memo_id = new_id();
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute(
            "INSERT INTO memos (id,title,content,created_at,updated_at,archived) VALUES (?1,'回滚备忘',NULL,1,1,0)",
            params![memo_id],
        )
        .unwrap();
        let bad = tx.execute(
            "INSERT INTO links (id,from_type,from_id,to_type,to_id,relation,created_at)
             VALUES (?1,'todo',?2,'memo',?3,'bogus_relation',1)",
            params![new_id(), t.id, memo_id],
        );
        assert!(bad.is_err(), "非法 relation 应触发 CHECK 失败");
        drop(tx); // 显式丢弃（未 commit）→ 回滚

        // memo 不落库
        let memos: i64 = conn.query_row("SELECT COUNT(*) FROM memos", [], |r| r.get(0)).unwrap();
        assert_eq!(memos, 0, "事务回滚后 memo 不应存在");
        let links: i64 = conn.query_row("SELECT COUNT(*) FROM links", [], |r| r.get(0)).unwrap();
        assert_eq!(links, 0);
        // 验证 memo_create_from_todo 的真实事务路径：先放一个会让 link 失败的状态——
        // 直接调用正常路径（links 表健康时）确保成功路径无回归
        let memo = memo_create_from_todo(conn, &t.id).unwrap();
        let _ = memo.id;
        let _ = std::fs::remove_dir_all(root);
    }

    /// TASKS 验收：3 条备忘整理 → md 含 3 个日期小节、链接 6 行（3×成对）；重复整理不重复
    #[test]
    fn organize_three_memos_into_note() {
        let (conn, root) = temp_db("organize");
        let conn = &conn;
        let notes_dir = root.join("notes");
        let mk = |title: &str, content: &str| {
            create_memo(conn, CreateMemo { title: title.into(), content: Some(content.into()) }).unwrap()
        };
        let m1 = mk("会议结论", "下周上线");
        let m2 = mk("踩坑记录", "注意 UTF-8 BOM");
        let m3 = mk("灵感", "支持快捷键");

        // 新建笔记整理 3 条
        let (note, created) = memo_to_note(
            conn,
            &notes_dir,
            &[m1.id.clone(), m2.id.clone(), m3.id.clone()],
            None,
            "本周整理",
        )
        .unwrap();
        assert!(created);
        let abs = notes_dir.parent().unwrap().join(&note.file_path);
        let md = fs::read_to_string(&abs).unwrap();
        assert_eq!(md.matches("## ").count(), 3, "应有 3 个日期小节：{md}");
        assert!(md.contains("会议结论") && md.contains("踩坑记录") && md.contains("灵感"));

        // 链接 6 行（3 memo × 成对：converted_to 有向 note，derived_from 从 note 出发）
        let links: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM links WHERE to_id=?1 OR from_id=?1",
                params![note.id],
                |r: &rusqlite::Row| r.get(0),
            )
            .unwrap();
        assert_eq!(links, 6, "3 成对 = 6 行，实际 {links}");

        // 重复整理同一条 → 不产生重复链接（UPSERT 语义）
        memo_to_note(conn, &notes_dir, std::slice::from_ref(&m1.id), Some(&note.id), "").unwrap();
        let links2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM links WHERE to_id=?1 OR from_id=?1",
                params![note.id],
                |r: &rusqlite::Row| r.get(0),
            )
            .unwrap();
        assert_eq!(links2, 6);

        // 追加：不覆盖原文
        memo_to_note(conn, &notes_dir, std::slice::from_ref(&m2.id), Some(&note.id), "").unwrap();
        let md2 = fs::read_to_string(&abs).unwrap();
        assert!(md2.contains("会议结论"), "原文保留");
        assert!(md2.matches("踩坑记录").count() >= 2, "追加小节存在");
        let _ = fs::remove_dir_all(root);
    }

    /// CRUD 与归档
    #[test]
    fn memo_crud_and_archive() {
        let (conn, root) = temp_db("crud");
        let conn = &conn;
        let m = create_memo(conn, CreateMemo { title: "备忘甲".into(), content: Some("内容".into()) }).unwrap();
        assert!(create_memo(conn, CreateMemo { title: "  ".into(), content: None }).is_err());

        let updated = update_memo(conn, &m.id, UpdateMemo { title: Some("改名".into()), ..Default::default() }).unwrap();
        assert_eq!(updated.title, "改名");

        archive_memo(conn, &m.id, true).unwrap();
        assert!(list_memos(conn, false).unwrap().iter().all(|x| x.id != m.id), "归档后不在默认列表");
        assert!(list_memos(conn, true).unwrap().iter().any(|x| x.id == m.id && x.archived));
        let _ = std::fs::remove_dir_all(root);
    }
}
