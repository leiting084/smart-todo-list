//! 笔记命令（F5/T2.2）：Markdown 文件存储 + DB 元数据。
//!
//! 存储约定（SPEC §5.3）：
//! - 正文存 `<data>/notes/<id>.md`；DB 只存元数据（title/file_path/updated_at）；
//! - 写文件走 `*.md.tmp` → `fs::rename` 原子替换（Windows 的 rename 带 REPLACE_EXISTING）；
//!   顺序=先写文件成功，再更新 DB（防"有库记录无文件"，TASKS 验收）；
//! - 打开时检测文件 mtime：晚于 DB updated_at → 视为外部修改，以文件内容为准并刷新 DB；
//! - 删除：md 移入 `notes/.trash/`（不物理删）+ DB 行 archived。

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Db;
use crate::id::new_id;
use crate::models::ItemType;

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    /// 相对 data/ 的路径，如 notes/<id>.md
    pub file_path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateNote {
    pub title: String,
    pub content: Option<String>,
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get("id")?,
        title: row.get("title")?,
        file_path: row.get("file_path")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        archived: row.get::<_, i64>("archived")? != 0,
    })
}

const COLUMNS: &str = "id, title, file_path, created_at, updated_at, archived";

// ---------- 文件操作 ----------

/// `<data>/notes/<id>.md` 的绝对路径（file_path 相对 data/）。
fn abs_path(notes_dir: &Path, rel_file_path: &str) -> PathBuf {
    notes_dir
        .parent()
        .map(|data| data.join(rel_file_path))
        .unwrap_or_else(|| notes_dir.join(rel_file_path))
}

/// 原子写：内容 → `*.md.tmp` → rename 替换。
fn atomic_write(target: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
    }
    let tmp = target.with_extension("md.tmp");
    fs::write(&tmp, content).map_err(|e| format!("写临时文件失败：{e}"))?;
    fs::rename(&tmp, target).map_err(|e| format!("原子替换失败：{e}"))
}

/// 文件 mtime（毫秒）；不存在返回 None。
fn mtime_ms(path: &Path) -> Option<i64> {
    fs::metadata(path).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis() as i64)
}

// ---------- repo（notes_dir 参数化便于单测） ----------

pub fn create_note(
    conn: &Connection,
    notes_dir: &Path,
    input: CreateNote,
) -> Result<Note, String> {
    if input.title.trim().is_empty() {
        return Err("笔记标题不能为空".into());
    }
    let id = new_id();
    let file_path = format!("notes/{id}.md");
    let abs = notes_dir.parent().map(|d| d.join(&file_path)).unwrap_or_else(|| notes_dir.join(&file_path));

    // 先文件后库：文件写失败直接报错，DB 不留记录
    atomic_write(&abs, input.content.as_deref().unwrap_or(""))?;
    let now = now_ms();
    conn.execute(
        "INSERT INTO notes (id,title,file_path,created_at,updated_at,archived) VALUES (?1,?2,?3,?4,?4,0)",
        params![id, input.title.trim(), file_path, now],
    )
    .map_err(|e| {
        // 回滚文件（尽力），保持"无库记录无文件"
        let _ = fs::remove_file(&abs);
        format!("创建笔记记录失败：{e}")
    })?;
    get_note(conn, notes_dir, &id)
}

pub fn get_note(conn: &Connection, notes_dir: &Path, id: &str) -> Result<Note, String> {
    let mut note = conn
        .query_row(&format!("SELECT {COLUMNS} FROM notes WHERE id=?1"), params![id], map_row)
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("笔记不存在：{id}"))?;
    // mtime 检测：文件比 DB 新 → 外部修改，以文件为准刷新 DB updated_at（SPEC §11）
    if let Some(m) = mtime_ms(&abs_path(notes_dir, &note.file_path)) {
        if m > note.updated_at {
            conn.execute(
                "UPDATE notes SET updated_at=?2 WHERE id=?1",
                params![note.id, m],
            )
            .map_err(|e| e.to_string())?;
            note.updated_at = m;
        }
    }
    Ok(note)
}

pub fn rename_note(conn: &Connection, notes_dir: &Path, id: &str, title: &str) -> Result<Note, String> {
    if get_note(conn, notes_dir, id).is_err() {
        return Err(format!("笔记不存在：{id}"));
    }
    if title.trim().is_empty() {
        return Err("笔记标题不能为空".into());
    }
    conn.execute(
        "UPDATE notes SET title=?2, updated_at=?3 WHERE id=?1",
        params![id, title.trim(), now_ms()],
    )
    .map_err(|e| e.to_string())?;
    get_note(conn, notes_dir, id)
}

/// 删除：md 移入 notes/.trash/（重名加时间戳），DB 行 archived。
pub fn delete_note(conn: &Connection, notes_dir: &Path, id: &str) -> Result<(), String> {
    let note = get_note(conn, notes_dir, id)?;
    let abs = abs_path(notes_dir, &note.file_path);
    if abs.exists() {
        let trash = notes_dir.join(".trash");
        fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let name = Path::new(&note.file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("note.md");
        let dest = trash.join(format!("{stamp}-{name}"));
        fs::rename(&abs, &dest).map_err(|e| format!("移入回收站失败：{e}"))?;
    }
    conn.execute(
        "UPDATE notes SET archived=1, updated_at=?2 WHERE id=?1",
        params![id, now_ms()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_notes(conn: &Connection, include_archived: bool) -> Result<Vec<Note>, String> {
    let sql = format!(
        "SELECT {COLUMNS} FROM notes {} ORDER BY updated_at DESC",
        if include_archived { "" } else { "WHERE archived=0" }
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], map_row).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 读正文（文件为准；外部修改过则刷新 DB updated_at）。
pub fn get_content(conn: &Connection, notes_dir: &Path, id: &str) -> Result<(Note, String), String> {
    let note = get_note(conn, notes_dir, id)?;
    let abs = abs_path(notes_dir, &note.file_path);
    let content = fs::read_to_string(&abs).map_err(|e| format!("读取笔记文件失败：{e}"))?;
    Ok((note, content))
}

/// 保存正文：先原子写文件，成功后更新 DB updated_at（先文件后库）+ wiki-link diff 同步。
pub fn save_content(conn: &Connection, notes_dir: &Path, id: &str, content: &str) -> Result<Note, String> {
    let note = get_note(conn, notes_dir, id)?;
    let abs = abs_path(notes_dir, &note.file_path);
    atomic_write(&abs, content)?;
    let now = now_ms();
    conn.execute("UPDATE notes SET updated_at=?2 WHERE id=?1", params![id, now])
        .map_err(|e| e.to_string())?;
    // T2.4：解析 [[todo:ID]]/[[memo:ID]]/[[note:ID]]，与 reference 链接 diff 同步（增/删）
    diff_sync_wiki_links(conn, id, content)?;
    get_note(conn, notes_dir, id)
}

/// 解析正文中的 wiki-link：`[[todo:ID]]` `[[memo:ID]]` `[[note:ID]]`。
/// 不引 regex：手写标记扫描。返回去重后的 (item_type, item_id) 列表（按出现序）。
pub fn parse_wiki_links(content: &str) -> Vec<(ItemType, String)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let markers = [
        ("[[todo:", ItemType::Todo),
        ("[[memo:", ItemType::Memo),
        ("[[note:", ItemType::Note),
    ];
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // 中文等多字节字符：+1 步进可能落在字符中间
        if !content.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let mut matched = false;
        for (marker, ty) in &markers {
            if content[i..].starts_with(marker) {
                if let Some(end_rel) = content[i + marker.len()..].find("]]") {
                    let raw_id = content[i + marker.len()..i + marker.len() + end_rel].trim().to_string();
                    if !raw_id.is_empty() && seen.insert((ty.as_str(), raw_id.clone())) {
                        out.push((*ty, raw_id));
                    }
                    i += marker.len() + end_rel + 2;
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            i += 1;
        }
    }
    out
}

/// wiki-link 与 reference 链接 diff 同步（from=note, relation='reference'）。
/// 目标实体不存在也保留 link（前端渲染红色失效态，SPEC F7）。
/// 不自开事务：调用方若在事务中（memo_to_note）则随外层提交；独立调用（save_content 命令）时
/// 各语句自提交——diff 幂等，中断后重保存自愈。
pub fn diff_sync_wiki_links(conn: &Connection, note_id: &str, content: &str) -> Result<(), String> {
    let parsed = parse_wiki_links(content);

    // 现有 reference（from=note）
    let mut existing: Vec<(ItemType, String)> = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT to_type, to_id FROM links WHERE from_type='note' AND from_id=?1 AND relation='reference'")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![note_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for r in rows {
            let (ty, tid) = r.map_err(|e| e.to_string())?;
            existing.push((ItemType::parse(&ty).unwrap_or(ItemType::Todo), tid));
        }
    }

    // 新增：parsed 中不在 existing
    for (ty, tid) in &parsed {
        if !existing.contains(&(*ty, tid.clone())) {
            let id = new_id();
            conn.execute(
                "INSERT INTO links (id,from_type,from_id,to_type,to_id,relation,created_at)
                 VALUES (?1,'note',?2,?3,?4,'reference',?5)",
                params![id, note_id, ty.as_str(), tid, now_ms()],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    // 删除：existing 中不在 parsed
    for (ty, tid) in &existing {
        if !parsed.contains(&(*ty, tid.clone())) {
            conn.execute(
                "DELETE FROM links WHERE from_type='note' AND from_id=?1 AND to_type=?2 AND to_id=?3 AND relation='reference'",
                params![note_id, ty.as_str(), tid],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 笔记生成待办（F6/T2.4）：建 todo（标题=选中文字截断）+ backlog(note→todo) + reference(todo→note)。
pub fn note_create_todo(
    conn: &Connection,
    note_id: &str,
    text: &str,
    date: Option<i64>,
) -> Result<crate::models::Todo, String> {
    // 笔记必须存在
    conn.query_row("SELECT 1 FROM notes WHERE id=?1", params![note_id], |_| Ok(()))
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("笔记不存在：{note_id}"))?;

    let title = text.trim();
    if title.is_empty() {
        return Err("待办标题不能为空".into());
    }
    // 截断过长标题（选中整段时）
    let truncated: String = if title.chars().count() > 50 {
        title.chars().take(50).collect()
    } else {
        title.to_string()
    };

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let todo_id = new_id();
    let now = now_ms();
    tx.execute(
        "INSERT INTO todos
           (id,title,content,completed,date,sort_order,color,category,priority,
            project_id,time,parent_id,postpone_auto,created_at,updated_at,completed_at)
         VALUES (?1,?2,NULL,0,?3,0,NULL,'life','medium',NULL,NULL,NULL,0,?4,?4,NULL)",
        params![todo_id, truncated, date, now],
    )
    .map_err(|e| format!("建待办失败：{e}"))?;

    // T4.5：同步 FTS 索引（此处绕开 create_todo 直写 todos 表，必须手动挂钩，
    // 否则"笔记生成待办"进来的数据搜不到。同步沿用「派生索引必须 grep 全写入路径」约定。）
    crate::services::search::sync_todo_fts(&tx, &todo_id)?;

    // backlog(note→todo) + reference(todo→note)
    let l1 = new_id();
    tx.execute(
        "INSERT INTO links (id,from_type,from_id,to_type,to_id,relation,created_at)
         VALUES (?1,'note',?2,'todo',?3,'backlog',?4)",
        params![l1, note_id, todo_id, now],
    )
    .map_err(|e| e.to_string())?;
    let l2 = new_id();
    tx.execute(
        "INSERT INTO links (id,from_type,from_id,to_type,to_id,relation,created_at)
         VALUES (?1,'todo',?2,'note',?3,'reference',?4)",
        params![l2, todo_id, note_id, now],
    )
    .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    crate::commands::todo::get_todo(conn, &todo_id)?.ok_or_else(|| "创建后查无此待办".into())
}

// ---------- Tauri 命令 ----------

fn notes_dir() -> &'static std::path::Path {
    &crate::paths::app_paths().notes_dir
}

#[tauri::command]
pub fn note_create(db: State<'_, Db>, input: CreateNote) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let note = create_note(&conn, notes_dir(), input)?;
    let _ = crate::services::oplog::record(&conn, "create", "note", Some(&note.id), &note.title);
    Ok(note)
}

#[tauri::command]
pub fn note_rename(db: State<'_, Db>, id: String, title: String) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let note = rename_note(&conn, notes_dir(), &id, &title)?;
    let _ = crate::services::oplog::record(
        &conn,
        "update",
        "note",
        Some(&note.id),
        &format!("重命名笔记：{}", note.title),
    );
    Ok(note)
}

#[tauri::command]
pub fn note_delete(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let title = get_note(&conn, notes_dir(), &id).map(|n| n.title).unwrap_or_else(|_| id.clone());
    delete_note(&conn, notes_dir(), &id)?;
    let _ = crate::services::oplog::record(&conn, "delete", "note", Some(&id), &title);
    Ok(())
}

#[tauri::command]
pub fn note_list(db: State<'_, Db>, include_archived: Option<bool>) -> Result<Vec<Note>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_notes(&conn, include_archived.unwrap_or(false))
}

#[tauri::command]
pub fn note_get(db: State<'_, Db>, id: String) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    get_note(&conn, notes_dir(), &id)
}

#[tauri::command]
pub fn note_get_content(db: State<'_, Db>, id: String) -> Result<NoteContent, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let (note, content) = get_content(&conn, notes_dir(), &id)?;
    Ok(NoteContent { note, content })
}

#[tauri::command]
pub fn note_save_content(db: State<'_, Db>, id: String, content: String) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    save_content(&conn, notes_dir(), &id, &content)
}

/// 笔记生成待办（F6/T2.4）。
#[tauri::command]
pub fn note_create_todo_cmd(
    db: State<'_, Db>,
    note_id: String,
    text: String,
    date: Option<i64>,
) -> Result<crate::models::Todo, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let todo = note_create_todo(&conn, &note_id, &text, date)?;
    let _ = crate::services::oplog::record(
        &conn,
        "create",
        "todo",
        Some(&todo.id),
        &format!("笔记生成待办：{}", todo.title),
    );
    Ok(todo)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteContent {
    pub note: Note,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CreateTodo;
    use std::path::PathBuf;

    fn temp(tag: &str) -> (Connection, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-note-test-{}-{}-{}",
            std::process::id(),
            std::line!(),
            tag
        ));
        let _ = fs::remove_dir_all(&root);
        let bkp = root.join("backups");
        fs::create_dir_all(&bkp).unwrap();
        let conn = crate::db::open_at(&root.join("todolist.db"), &bkp).unwrap();
        (conn, root.join("notes"), root)
    }

    fn note(conn: &Connection, notes_dir: &Path, title: &str, content: &str) -> Note {
        create_note(conn, notes_dir, CreateNote { title: title.into(), content: Some(content.into()) }).unwrap()
    }

    /// 创建 → 文件存在 + 库记录；保存中途杀进程（模拟：只写 tmp 不 rename）不产生"有库记录无文件"
    #[test]
    fn create_writes_file_and_meta_atomic() {
        let (conn, notes_dir, root) = temp("create");
        let conn = &conn;
        let n = note(conn, &notes_dir, "笔记甲", "# 正文");
        let abs = notes_dir.parent().unwrap().join(&n.file_path);
        assert!(abs.exists(), "md 文件应存在");
        assert_eq!(fs::read_to_string(&abs).unwrap(), "# 正文");
        // 无 tmp 残留
        assert!(!notes_dir.parent().unwrap().join(format!("{}.md.tmp", n.id)).exists());

        // 模拟保存中途崩溃：手写 tmp 但未 rename → 库与正式文件不受影响
        fs::write(abs.with_extension("md.tmp"), "半截").unwrap();
        let (n2, content) = get_content(conn, &notes_dir, &n.id).unwrap();
        assert_eq!(content, "# 正文", "崩溃残留 tmp 不影响正文");
        drop(n2);
        let _ = fs::remove_dir_all(root);
    }

    /// TASKS 验收：外部修改 md 后重新打开，以文件内容为准（mtime > DB updated_at）
    #[test]
    fn external_edit_wins_with_mtime_check() {
        let (conn, notes_dir, root) = temp("external");
        let conn = &conn;
        let n = note(conn, &notes_dir, "外部编辑", "旧内容");
        let abs = notes_dir.parent().unwrap().join(&n.file_path);

        // 等待确保 mtime 严格大于 created（FAT 精度）——内容改写并推进时间
        std::thread::sleep(std::time::Duration::from_millis(40));
        fs::write(&abs, "外部修改后的新内容").unwrap();

        let (note2, content) = get_content(conn, &notes_dir, &n.id).unwrap();
        assert_eq!(content, "外部修改后的新内容", "以文件内容为准");
        // DB updated_at 已刷新到文件 mtime
        assert!(note2.updated_at >= n.updated_at);
        let _ = fs::remove_dir_all(root);
    }

    /// 删除：md 移入 .trash，库行归档
    #[test]
    fn delete_moves_file_to_trash() {
        let (conn, notes_dir, root) = temp("delete");
        let conn = &conn;
        let n = note(conn, &notes_dir, "将删除", "内容");
        let abs = notes_dir.parent().unwrap().join(&n.file_path);
        assert!(abs.exists());

        delete_note(conn, &notes_dir, &n.id).unwrap();
        assert!(!abs.exists(), "原位置文件应移走");
        let trash_dir = notes_dir.join(".trash");
        let trashed: Vec<_> = fs::read_dir(&trash_dir).unwrap().collect();
        assert_eq!(trashed.len(), 1, "trash 应有 1 份");
        assert!(list_notes(conn, false).unwrap().iter().all(|x| x.id != n.id));
        assert!(list_notes(conn, true).unwrap().iter().any(|x| x.id == n.id && x.archived));
        let _ = fs::remove_dir_all(root);
    }

    /// save 后库与文件一致 + list 排序（updated_at DESC）
    #[test]
    fn save_updates_both_and_list_orders() {
        let (conn, notes_dir, root) = temp("save");
        let conn = &conn;
        let a = note(conn, &notes_dir, "A", "a");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let b = note(conn, &notes_dir, "B", "b");

        save_content(conn, &notes_dir, &a.id, "a2").unwrap();
        let (_, content) = get_content(conn, &notes_dir, &a.id).unwrap();
        assert_eq!(content, "a2");
        // a 保存后 updated_at > b 创建 → list 第一位是 A
        let list = list_notes(conn, false).unwrap();
        assert_eq!(list[0].title, "A", "最近更新的在前");
        let _ = b;
        let _ = fs::remove_dir_all(root);
    }

    /// wiki-link 解析：三种类型 + 去重 + 残缺容忍
    #[test]
    fn parse_wiki_links_basic() {
        let links = parse_wiki_links(
            "见 [[todo:123]] 和 [[memo:abc]] 与 [[note:def]]，重复 [[todo:123]]，残缺 [[todo:",
        );
        assert_eq!(links.len(), 3, "去重+忽略残缺：{links:?}");
        assert!(links.contains(&(ItemType::Todo, "123".into())));
        assert!(links.contains(&(ItemType::Memo, "abc".into())));
        assert!(links.contains(&(ItemType::Note, "def".into())));
        assert_eq!(parse_wiki_links("没有链接").len(), 0);
    }

    /// TASKS 验收：含 3 个 wiki-link 保存后 links 有 3 行；删掉一个再保存变 2 行
    #[test]
    fn save_syncs_reference_links() {
        let (conn, notes_dir, root) = temp("wiki");
        let conn = &conn;
        let n = note(conn, &notes_dir, "反链笔记", "占位");
        let t = crate::commands::todo::create_todo(conn, CreateTodo {
            title: "任务一".into(),
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
        .unwrap();

        // 含 3 个 wiki-link（一个指向不存在的 todo:ghost——保留失效）
        save_content(
            conn,
            &notes_dir,
            &n.id,
            "看 [[todo:GHOST1]]、[[memo:GHOST2]]、[[note:GHOST3]] 和 [[todo:GHOST1]] 重复",
        )
        .unwrap();
        let refs: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT to_type, to_id FROM links WHERE from_type='note' AND from_id=?1 AND relation='reference'")
                .unwrap();
            let rows = stmt.query_map(params![n.id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        // 去重后 3 个目标（ghost 重复算 1）
        assert_eq!(refs.len(), 3, "去重后应有 3 条 reference：{refs:?}");

        // 删掉 2 个 link（保留 todo:GHOST1）再保存 → reference 变 1 行
        save_content(conn, &notes_dir, &n.id, "只剩 [[todo:GHOST1]]").unwrap();
        let refs2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM links WHERE from_type='note' AND from_id=?1 AND relation='reference'",
                params![n.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(refs2, 1, "diff 删除后应剩 1：实际 {refs2}");

        let _ = fs::remove_dir_all(root);
        let _ = t;
    }

    /// TASKS 验收：note_create_todo → backlog(note→todo) + reference(todo→note) + 截断
    #[test]
    fn note_create_todo_creates_links() {
        let (conn, notes_dir, root) = temp("createtodo");
        let conn = &conn;
        let n = note(conn, &notes_dir, "来源笔记", "");
        let long_text = "很长的选中文字".repeat(20); // 140 字
        let t = note_create_todo(conn, &n.id, &long_text, Some(20260920)).unwrap();
        assert_eq!(t.title.chars().count(), 50, "标题应截断到 50 字");
        assert_eq!(t.date, Some(20260920));

        let links = crate::commands::link::links_for(conn, crate::models::ItemType::Todo, &t.id).unwrap();
        assert!(links.iter().any(|l| l.relation == "backlog" && l.from_id == n.id));
        assert!(links.iter().any(|l| l.relation == "reference" && l.to_id == n.id));

        // 空文本拒绝
        assert!(note_create_todo(conn, &n.id, "  ", None).is_err());
        // 不存在笔记拒绝
        assert!(note_create_todo(conn, "ghost", "x", None).is_err());
        let _ = fs::remove_dir_all(root);
    }

    /// G1 缺口回归：note_create_todo 绕过 create_todo 裸写 todos，必须自己同步 FTS
    #[test]
    fn note_create_todo_syncs_fts() {
        let (conn, notes_dir, root) = temp("createtodofts");
        let conn = &conn;
        let n = note(conn, &notes_dir, "来源笔记", "");
        let t = note_create_todo(conn, &n.id, "从笔记提取的新待办", Some(20260920)).unwrap();

        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM todos_fts WHERE id=?1", params![t.id], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, 1, "笔记生成的待办必须进 FTS 索引");

        let (title, content): (String, String) = conn
            .query_row("SELECT title, content FROM todos_fts WHERE id=?1", params![t.id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(title, t.title);
        assert_eq!(content, "", "content 为空串而非 NULL");
        let _ = fs::remove_dir_all(root);
    }
}
