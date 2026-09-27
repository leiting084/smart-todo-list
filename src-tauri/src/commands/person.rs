//! 人员与任务分配命令（F31，继承旧版）：person CRUD + todo_set_assignees + 协作完成。
//!
//! 协作完成语义（沿用旧版 assignedTo/completedBy，TASKS T1.6 验收）：
//! - 多人分配：每人一条 todo_assignees 完成标记，各自勾选只改自己；
//!   **全部 assignee 都勾选 → todos.completed=1**；任一人取消 → 整体回到未完成。
//! - 单人/无分配：直接勾选即整体完成（todo_set_completed）。
//! - 变更分配集：被移除者的完成标记随行删除，然后按剩余标记重算整体状态。

use rusqlite::{params, types::Value, Connection, Row};
use tauri::State;

use crate::db::Db;
use crate::id::new_id;
use crate::models::{Assignee, CreatePerson, Person, UpdatePerson};

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

fn map_row(row: &Row<'_>) -> rusqlite::Result<Person> {
    Ok(Person {
        id: row.get("id")?,
        name: row.get("name")?,
        note: row.get("note")?,
        role: row.get("role")?,
        email: row.get("email")?,
        sort_order: row.get("sort_order")?,
        archived: row.get::<_, i64>("archived")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

const COLUMNS: &str = "id, name, note, role, email, sort_order, archived, created_at, updated_at";

// ---------- 人员 repo ----------

pub fn create_person(conn: &Connection, input: CreatePerson) -> Result<Person, String> {
    if input.name.trim().is_empty() {
        return Err("人员姓名不能为空".into());
    }
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO people (id,name,note,role,email,sort_order,archived,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,0,?7,?7)",
        params![
            id,
            input.name.trim(),
            input.note,
            input.role,
            input.email,
            input.sort_order.unwrap_or(0),
            now
        ],
    )
    .map_err(|e| format!("创建人员失败：{e}"))?;
    get_person(conn, &id)
}

pub fn get_person(conn: &Connection, id: &str) -> Result<Person, String> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM people WHERE id=?1"), params![id], map_row)
        .map_err(|e| e.to_string())
}

pub fn update_person(conn: &Connection, id: &str, input: UpdatePerson) -> Result<Person, String> {
    if get_person(conn, id).is_err() {
        return Err(format!("人员不存在：{id}"));
    }
    if let Some(n) = &input.name {
        if n.trim().is_empty() {
            return Err("人员姓名不能为空".into());
        }
    }
    let mut sets: Vec<String> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    let mut push = |col: &str, v: Value| {
        sets.push(format!("{col} = ?{}", vals.len() + 1));
        vals.push(v);
    };
    if let Some(n) = input.name {
        push("name", Value::Text(n.trim().to_string()));
    }
    if let Some(n) = input.note {
        push("note", n.map_or(Value::Null, Value::Text));
    }
    if let Some(r) = input.role {
        push("role", r.map_or(Value::Null, Value::Text));
    }
    if let Some(e) = input.email {
        push("email", e.map_or(Value::Null, Value::Text));
    }
    if let Some(s) = input.sort_order {
        push("sort_order", Value::Integer(s));
    }
    if !sets.is_empty() {
        sets.push(format!("updated_at = ?{}", vals.len() + 1));
        vals.push(Value::Integer(now_ms()));
        vals.push(Value::Text(id.to_string()));
        conn.execute(
            &format!("UPDATE people SET {} WHERE id=?{}", sets.join(", "), vals.len()),
            rusqlite::params_from_iter(vals),
        )
        .map_err(|e| e.to_string())?;
    }
    get_person(conn, id)
}

/// 归档/恢复（历史记录保留；归档后不在选择器出现）。
pub fn set_person_archived(conn: &Connection, id: &str, archived: bool) -> Result<(), String> {
    let n = conn
        .execute(
            "UPDATE people SET archived=?2, updated_at=?3 WHERE id=?1",
            params![id, archived as i64, now_ms()],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("人员不存在：{id}"));
    }
    Ok(())
}

pub fn list_people(conn: &Connection, include_archived: bool) -> Result<Vec<Person>, String> {
    let sql = format!(
        "SELECT {COLUMNS} FROM people {} ORDER BY sort_order ASC, created_at ASC",
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

// ---------- 分配与协作完成 ----------

/// 一条待办的分配状态（含每人姓名）。
pub fn assignees_of(conn: &Connection, todo_id: &str) -> Result<Vec<Assignee>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT ta.person_id, COALESCE(p.name, '(已删除)'), ta.completed
             FROM todo_assignees ta LEFT JOIN people p ON p.id = ta.person_id
             WHERE ta.todo_id = ?1 ORDER BY p.sort_order, p.created_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![todo_id], |r| {
            Ok(Assignee {
                person_id: r.get(0)?,
                name: r.get(1)?,
                completed: r.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 按当前 todo_assignees 的完成标记重算整体 completed/completed_at。
/// 规则：无分配 → 不动（整体勾选走 todo_set_completed）；有分配 → 全勾选=完成。
fn recompute_overall(conn: &Connection, todo_id: &str) -> Result<(), String> {
    let (total, done): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(completed),0) FROM todo_assignees WHERE todo_id=?1",
            params![todo_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let now = now_ms();
    if total > 0 && total == done {
        conn.execute(
            "UPDATE todos SET completed=1, completed_at=COALESCE(completed_at,?2), updated_at=?2 WHERE id=?1",
            params![todo_id, now],
        )
        .map_err(|e| e.to_string())?;
    } else if total > 0 {
        conn.execute(
            "UPDATE todos SET completed=0, completed_at=NULL, updated_at=?2 WHERE id=?1",
            params![todo_id, now],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 整体勾选入口：把"我"的意图落到分配标记上（单人/多人都先同步到 todo_assignees 再重算）。
/// todo_id 有分配者时用这个；无分配者走 todo_set_completed。
pub fn person_toggle(conn: &Connection, todo_id: &str, person_id: &str, completed: bool) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let n = tx
        .execute(
            "UPDATE todo_assignees SET completed=?3, completed_at=?4 WHERE todo_id=?1 AND person_id=?2",
            params![todo_id, person_id, completed as i64, completed.then(now_ms)],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("该人员未被分配此待办：{person_id}"));
    }
    recompute_overall(&tx, todo_id)?;
    tx.commit().map_err(|e| e.to_string())
}

/// 设置分配人集合（单事务）：全量替换 + 按剩余标记重算整体状态。
pub fn set_assignees(
    conn: &Connection,
    todo_id: &str,
    person_ids: &[String],
) -> Result<Vec<Assignee>, String> {
    // 待办必须存在
    let exists: i64 = conn
        .query_row("SELECT COUNT(*) FROM todos WHERE id=?1", params![todo_id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        return Err(format!("待办不存在：{todo_id}"));
    }
    // 人员必须都存在且未归档
    for pid in person_ids {
        let p = get_person(conn, pid)?;
        if p.archived {
            return Err(format!("人员已归档，不能分配：{}", p.name));
        }
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    // 先读旧标记：整体替换时保留未变动人员的完成状态（只清被移除者）
    let old_marks: Vec<(String, i64, Option<i64>)> = {
        let mut stmt = tx
            .prepare("SELECT person_id, completed, completed_at FROM todo_assignees WHERE todo_id=?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![todo_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?;
        let mut v = Vec::new();
        for r in rows {
            v.push(r.map_err(|e| e.to_string())?);
        }
        v
    };
    tx.execute("DELETE FROM todo_assignees WHERE todo_id=?1", params![todo_id])
        .map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO todo_assignees (todo_id, person_id, completed, completed_at)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(|e| e.to_string())?;
        // 去重，保持顺序；旧分配者继承其完成标记
        let mut seen = std::collections::HashSet::new();
        for pid in person_ids {
            if !seen.insert(pid.clone()) {
                continue;
            }
            let prev = old_marks.iter().find(|(old_pid, _, _)| old_pid == pid);
            let (done, at) = prev.map_or((0, None), |(_, c, a)| (*c, *a));
            stmt.execute(params![todo_id, pid, done, at])
                .map_err(|e| e.to_string())?;
        }
    }
    // 被移除者标记随 DELETE 消失；按剩余标记重算整体状态
    recompute_overall(&tx, todo_id)?;
    tx.commit().map_err(|e| e.to_string())?;
    assignees_of(conn, todo_id)
}

// ---------- Tauri 命令 ----------

#[tauri::command]
pub fn person_create(db: State<'_, Db>, input: CreatePerson) -> Result<Person, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let person = create_person(&conn, input)?;
    let _ = crate::services::oplog::record(&conn, "create", "person", Some(&person.id), &person.name);
    Ok(person)
}

#[tauri::command]
pub fn person_update(db: State<'_, Db>, id: String, input: UpdatePerson) -> Result<Person, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let person = update_person(&conn, &id, input)?;
    let _ = crate::services::oplog::record(&conn, "update", "person", Some(&person.id), &person.name);
    Ok(person)
}

#[tauri::command]
pub fn person_archive(db: State<'_, Db>, id: String, archived: bool) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let name = get_person(&conn, &id).map(|p| p.name).unwrap_or_else(|_| id.clone());
    set_person_archived(&conn, &id, archived)?;
    let action = if archived { "archive" } else { "unarchive" };
    let _ = crate::services::oplog::record(&conn, action, "person", Some(&id), &name);
    Ok(())
}

#[tauri::command]
pub fn person_list(db: State<'_, Db>, include_archived: Option<bool>) -> Result<Vec<Person>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_people(&conn, include_archived.unwrap_or(false))
}

#[tauri::command]
pub fn todo_get_assignees(db: State<'_, Db>, todo_id: String) -> Result<Vec<Assignee>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    assignees_of(&conn, &todo_id)
}

#[tauri::command]
pub fn todo_set_assignees(
    db: State<'_, Db>,
    todo_id: String,
    person_ids: Vec<String>,
) -> Result<Vec<Assignee>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    set_assignees(&conn, &todo_id, &person_ids)
}

/// 分配人自己的勾选（协作完成语义）。
#[tauri::command]
pub fn todo_person_toggle(
    db: State<'_, Db>,
    todo_id: String,
    person_id: String,
    completed: bool,
) -> Result<Vec<Assignee>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    person_toggle(&conn, &todo_id, &person_id, completed)?;
    assignees_of(&conn, &todo_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::todo::{create_todo, get_todo};
    use crate::models::CreateTodo;

    fn temp_db(tag: &str) -> (Connection, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-person-test-{}-{}-{}",
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

    fn person(conn: &Connection, name: &str) -> Person {
        create_person(conn, CreatePerson { name: name.into(), ..Default::default() }).unwrap()
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

    /// 张三+李四：张三勾自己→整体未完成；李四再勾→整体完成（TASKS 验收原文场景）
    #[test]
    fn two_assignees_collaborative_completion() {
        let (conn, root) = temp_db("collab");
        let zhang = person(&conn, "张三");
        let li = person(&conn, "李四");
        let t = todo(&conn, "协作任务");
        let list = set_assignees(&conn, &t.id, &[zhang.id.clone(), li.id.clone()]).unwrap();
        assert_eq!(list.len(), 2);
        // 设置分配后整体回到未完成
        assert!(!get_todo(&conn, &t.id).unwrap().unwrap().completed);

        // 张三勾自己 → 整体仍未完成
        person_toggle(&conn, &t.id, &zhang.id, true).unwrap();
        let overall = get_todo(&conn, &t.id).unwrap().unwrap();
        assert!(!overall.completed, "张三勾完李四未勾，整体必须未完成");
        let marks = assignees_of(&conn, &t.id).unwrap();
        assert_eq!(
            marks.iter().map(|a| (a.name.as_str(), a.completed)).collect::<Vec<_>>(),
            vec![("张三", true), ("李四", false)]
        );

        // 李四再勾 → 整体完成
        person_toggle(&conn, &t.id, &li.id, true).unwrap();
        let overall = get_todo(&conn, &t.id).unwrap().unwrap();
        assert!(overall.completed, "全部分配人完成 → 整体完成");
        assert!(overall.completed_at.is_some());

        // 李四取消 → 整体回到未完成、completed_at 清空
        person_toggle(&conn, &t.id, &li.id, false).unwrap();
        let overall = get_todo(&conn, &t.id).unwrap().unwrap();
        assert!(!overall.completed);
        assert!(overall.completed_at.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    /// 单人分配 = 勾自己即整体完成
    #[test]
    fn single_assignee_direct_completion() {
        let (conn, root) = temp_db("single");
        let wang = person(&conn, "王五");
        let t = todo(&conn, "单人任务");
        set_assignees(&conn, &t.id, std::slice::from_ref(&wang.id)).unwrap();
        person_toggle(&conn, &t.id, &wang.id, true).unwrap();
        assert!(get_todo(&conn, &t.id).unwrap().unwrap().completed);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 取消某人分配 → 其完成标记一并清除，整体状态按剩余重算
    #[test]
    fn removing_assignee_clears_his_mark_and_recomputes() {
        let (conn, root) = temp_db("remove");
        let a = person(&conn, "A");
        let b = person(&conn, "B");
        let t = todo(&conn, "任务");
        set_assignees(&conn, &t.id, &[a.id.clone(), b.id.clone()]).unwrap();
        person_toggle(&conn, &t.id, &a.id, true).unwrap();
        person_toggle(&conn, &t.id, &b.id, true).unwrap();
        assert!(get_todo(&conn, &t.id).unwrap().unwrap().completed);

        // 移除 B（A 保持勾选）→ B 标记消失；剩余只有 A 且已勾 → 整体仍完成
        let after = set_assignees(&conn, &t.id, std::slice::from_ref(&a.id)).unwrap();
        assert_eq!(after.len(), 1);
        assert!(after[0].completed, "A 的标记保留");
        assert!(get_todo(&conn, &t.id).unwrap().unwrap().completed);

        // 全部移除 → todo_assignees 空；整体状态不再由分配驱动（不动 completed）
        set_assignees(&conn, &t.id, &[]).unwrap();
        assert!(assignees_of(&conn, &t.id).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    /// 归档人员：不进默认列表、不能再分配、历史标记保留
    #[test]
    fn archived_person_keeps_history() {
        let (conn, root) = temp_db("arch");
        let p = person(&conn, "老王");
        let t = todo(&conn, "历史任务");
        set_assignees(&conn, &t.id, std::slice::from_ref(&p.id)).unwrap();
        person_toggle(&conn, &t.id, &p.id, true).unwrap();

        set_person_archived(&conn, &p.id, true).unwrap();
        assert!(list_people(&conn, false).unwrap().iter().all(|x| x.id != p.id));
        assert!(list_people(&conn, true).unwrap().iter().any(|x| x.id == p.id && x.archived));

        let t2 = todo(&conn, "新任务");
        assert!(
            set_assignees(&conn, &t2.id, std::slice::from_ref(&p.id)).is_err(),
            "归档人员不能再分配"
        );
        // 历史标记仍在
        let marks = assignees_of(&conn, &t.id).unwrap();
        assert_eq!(marks.len(), 1);
        assert!(marks[0].completed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn person_validation() {
        let (conn, root) = temp_db("valid");
        assert!(create_person(&conn, CreatePerson { name: "  ".into(), ..Default::default() }).is_err());
        let p = person(&conn, "张三");
        assert!(update_person(&conn, &p.id, UpdatePerson { name: Some("".into()), ..Default::default() }).is_err());
        assert!(update_person(&conn, "ghost", UpdatePerson::default()).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    /// 0004 people_profile：role/email 创建回读、缺失不改、显式 null 清空、落库列验证
    #[test]
    fn role_email_create_update_and_clear() {
        let (conn, root) = temp_db("profile");
        let p = create_person(
            &conn,
            CreatePerson {
                name: "张三".into(),
                role: Some("前端开发".into()),
                email: Some("<email@example.com>".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.role.as_deref(), Some("前端开发"));
        assert_eq!(p.email.as_deref(), Some("<email@example.com>"));

        // 缺失字段 = 不改（note/role/email 均保持）
        let same = update_person(
            &conn,
            &p.id,
            UpdatePerson { note: Some(Some("备注".into())), ..Default::default() },
        )
        .unwrap();
        assert_eq!(same.role.as_deref(), Some("前端开发"));
        assert_eq!(same.email.as_deref(), Some("<email@example.com>"));
        assert_eq!(same.note.as_deref(), Some("备注"));

        // 显式 null = 清空 email（role 不动）
        let cleared = update_person(&conn, &p.id, UpdatePerson { email: Some(None), ..Default::default() }).unwrap();
        assert_eq!(cleared.email, None);
        assert_eq!(cleared.role.as_deref(), Some("前端开发"));

        // 落库列直接验证（list/get 之外的真相源）
        let (role, email): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT role, email FROM people WHERE id=?1",
                params![p.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(role.as_deref(), Some("前端开发"));
        assert_eq!(email, None);

        // update 也能改写 role
        let renamed = update_person(
            &conn,
            &p.id,
            UpdatePerson { role: Some(Some("后端开发".into())), ..Default::default() },
        )
        .unwrap();
        assert_eq!(renamed.role.as_deref(), Some("后端开发"));
        let _ = std::fs::remove_dir_all(root);
    }
}
