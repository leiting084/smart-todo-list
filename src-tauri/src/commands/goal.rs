//! 长期目标命令（F32，继承旧版）：goal CRUD + goal_set_links + 进度统计。
//!
//! goal_links 是多态关联（item_type+item_id 无外键），悬挂处理策略：
//! - 删除实体时**事务清理** goal_links 与 item_tags（避免污染进度统计/标签计数）；
//! - links（wiki 反链）**有意保留悬挂**，前端显示失效态（SPEC T2.4）。

use rusqlite::{params, types::Value, Connection, OptionalExtension, Row};
use tauri::State;

use crate::db::Db;
use crate::id::new_id;
use crate::models::{CreateGoal, Goal, GoalLink, GoalLinkInput, GoalStats, ItemType, UpdateGoal};

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

fn map_row(row: &Row<'_>) -> rusqlite::Result<Goal> {
    Ok(Goal {
        id: row.get("id")?,
        title: row.get("title")?,
        category: row.get("category")?,
        description: row.get("description")?,
        status: row.get("status")?,
        progress: row.get("progress")?,
        target_date: row.get("target_date")?,
        // 计算字段占位，读取后由 fill_link_counts 按 goal_links 回填（F32 主进度）
        link_progress: 0.0,
        todo_count: 0,
        done_count: 0,
        sort_order: row.get("sort_order")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

const COLUMNS: &str =
    "id, title, category, description, status, progress, target_date, sort_order, created_at, updated_at";
const STATUSES: [&str; 3] = ["active", "done", "archived"];

fn validate(title: &str, status: &str) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("目标标题不能为空".into());
    }
    if !STATUSES.contains(&status) {
        return Err(format!("status 必须是 {STATUSES:?}，实际 {status}"));
    }
    Ok(())
}

/// 手动进度钳制到 0-100。
fn clamp_progress(p: i64) -> i64 {
    p.clamp(0, 100)
}

// ---------- CRUD ----------

pub fn create_goal(conn: &Connection, input: CreateGoal) -> Result<Goal, String> {
    validate(&input.title, "active")?;
    let id = new_id();
    let now = now_ms();
    let progress = clamp_progress(input.progress.unwrap_or(0));
    conn.execute(
        "INSERT INTO goals (id,title,category,description,status,progress,target_date,sort_order,created_at,updated_at)
         VALUES (?1,?2,?3,?4,'active',?5,?6,?7,?8,?8)",
        params![
            id,
            input.title.trim(),
            input.category,
            input.description,
            progress,
            input.target_date,
            input.sort_order.unwrap_or(0),
            now
        ],
    )
    .map_err(|e| format!("创建目标失败：{e}"))?;
    get_goal(conn, &id)
}

/// 回填目标的关联待办计数与完成率（F32：目标进度=关联待办完成率）。
fn fill_link_counts(conn: &Connection, goal: &mut Goal) -> rusqlite::Result<()> {
    let (total, done): (i64, i64) = conn.query_row(
        "SELECT COUNT(t.id), COALESCE(SUM(t.completed),0)
         FROM goal_links gl JOIN todos t ON t.id = gl.item_id
         WHERE gl.goal_id=?1 AND gl.item_type='todo'",
        params![goal.id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    goal.todo_count = total;
    goal.done_count = done;
    goal.link_progress = if total == 0 { 0.0 } else { done as f64 / total as f64 };
    Ok(())
}

pub fn get_goal(conn: &Connection, id: &str) -> Result<Goal, String> {
    let mut goal = conn
        .query_row(&format!("SELECT {COLUMNS} FROM goals WHERE id=?1"), params![id], map_row)
        .map_err(|e| e.to_string())?;
    fill_link_counts(conn, &mut goal).map_err(|e| e.to_string())?;
    Ok(goal)
}

pub fn update_goal(conn: &Connection, id: &str, input: UpdateGoal) -> Result<Goal, String> {
    if get_goal(conn, id).is_err() {
        return Err(format!("目标不存在：{id}"));
    }
    if let Some(t) = &input.title {
        if t.trim().is_empty() {
            return Err("目标标题不能为空".into());
        }
    }
    if let Some(s) = &input.status {
        if !STATUSES.contains(&s.as_str()) {
            return Err(format!("status 必须是 {STATUSES:?}，实际 {s}"));
        }
    }
    let mut sets: Vec<String> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    let mut push = |col: &str, v: Value| {
        sets.push(format!("{col} = ?{}", vals.len() + 1));
        vals.push(v);
    };
    if let Some(t) = input.title {
        push("title", Value::Text(t.trim().to_string()));
    }
    if let Some(c) = input.category {
        push("category", c.map_or(Value::Null, Value::Text));
    }
    if let Some(d) = input.description {
        push("description", d.map_or(Value::Null, Value::Text));
    }
    if let Some(s) = input.status {
        push("status", Value::Text(s));
    }
    if let Some(p) = input.progress {
        push("progress", Value::Integer(clamp_progress(p)));
    }
    if let Some(td) = input.target_date {
        push("target_date", td.map_or(Value::Null, Value::Integer));
    }
    if let Some(o) = input.sort_order {
        push("sort_order", Value::Integer(o));
    }
    if !sets.is_empty() {
        sets.push(format!("updated_at = ?{}", vals.len() + 1));
        vals.push(Value::Integer(now_ms()));
        vals.push(Value::Text(id.to_string()));
        conn.execute(
            &format!("UPDATE goals SET {} WHERE id=?{}", sets.join(", "), vals.len()),
            rusqlite::params_from_iter(vals),
        )
        .map_err(|e| e.to_string())?;
    }
    get_goal(conn, id)
}

/// 删除=归档（goals.status='archived'）。
pub fn archive_goal(conn: &Connection, id: &str) -> Result<(), String> {
    update_goal(
        conn,
        id,
        UpdateGoal {
            status: Some("archived".into()),
            ..Default::default()
        },
    )?;
    Ok(())
}

pub fn list_goals(conn: &Connection, include_archived: bool) -> Result<Vec<Goal>, String> {
    let sql = format!(
        "SELECT {COLUMNS} FROM goals {} ORDER BY sort_order ASC, created_at ASC",
        if include_archived { "" } else { "WHERE status != 'archived'" }
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], map_row).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    for g in out.iter_mut() {
        fill_link_counts(conn, g).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

// ---------- 关联 ----------

/// 校验关联实体存在（防挂空）。
fn assert_item_exists(conn: &Connection, ty: ItemType, item_id: &str) -> Result<(), String> {
    let table = match ty {
        ItemType::Todo => "todos",
        ItemType::Memo => "memos",
        ItemType::Note => "notes",
    };
    let n: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM {table} WHERE id=?1"), params![item_id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("关联目标失败：{table} 中不存在 {item_id}"));
    }
    Ok(())
}

/// 全量替换目标的关联集合（单事务）。
pub fn set_goal_links(
    conn: &Connection,
    goal_id: &str,
    items: &[GoalLinkInput],
) -> Result<Vec<GoalLink>, String> {
    let goal = get_goal(conn, goal_id)?;
    if goal.status == "archived" {
        return Err("目标已归档，不能再修改关联".into());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    // 校验全部存在后再动库
    for it in items {
        assert_item_exists(&tx, it.item_type, &it.item_id)?;
    }
    tx.execute("DELETE FROM goal_links WHERE goal_id=?1", params![goal_id])
        .map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR IGNORE INTO goal_links (goal_id, item_type, item_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(|e| e.to_string())?;
        let mut seen = std::collections::HashSet::new();
        let now = now_ms();
        for it in items {
            let key = (it.item_type.as_str().to_string(), it.item_id.clone());
            if seen.insert(key) {
                stmt.execute(params![goal_id, it.item_type.as_str(), it.item_id, now])
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    get_goal_links(conn, goal_id)
}

/// 目标的关联列表（JOIN 各表取标题；实体已删的显示悬挂 title=None）。
pub fn get_goal_links(conn: &Connection, goal_id: &str) -> Result<Vec<GoalLink>, String> {
    let rows_sql = "SELECT item_type, item_id FROM goal_links WHERE goal_id=?1 ORDER BY item_type, item_id";
    let mut stmt = conn.prepare(rows_sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![goal_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut raw: Vec<(String, String)> = Vec::new();
    for r in rows {
        raw.push(r.map_err(|e| e.to_string())?);
    }

    let mut out = Vec::new();
    for (ty, item_id) in raw {
        let item_type = ItemType::parse(&ty)?;
        let title: Option<String> = match item_type {
            ItemType::Todo => conn
                .query_row("SELECT title FROM todos WHERE id=?1", params![item_id], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?,
            ItemType::Memo => conn
                .query_row("SELECT title FROM memos WHERE id=?1", params![item_id], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?,
            ItemType::Note => conn
                .query_row("SELECT title FROM notes WHERE id=?1", params![item_id], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?,
        };
        let completed = if item_type == ItemType::Todo {
            conn.query_row("SELECT completed FROM todos WHERE id=?1", params![item_id], |r| {
                r.get::<_, i64>(0).map(|v| v != 0)
            })
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(false)
        } else {
            false
        };
        out.push(GoalLink {
            item_type,
            item_id,
            title,
            completed,
        });
    }
    Ok(out)
}

/// 目标关联统计（仅供参考）：progress=关联待办完成率。
/// 目标主进度是 goals.progress（手动 0-100，0005），与此字段相互独立。
pub fn goal_stats(conn: &Connection, goal_id: &str) -> Result<GoalStats, String> {
    let (total, done): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(t.completed),0)
             FROM goal_links gl JOIN todos t ON t.id = gl.item_id
             WHERE gl.goal_id=?1 AND gl.item_type='todo'",
            params![goal_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let memo_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM goal_links gl JOIN memos m ON m.id = gl.item_id
             WHERE gl.goal_id=?1 AND gl.item_type='memo'",
            params![goal_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let note_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM goal_links gl JOIN notes n ON n.id = gl.item_id
             WHERE gl.goal_id=?1 AND gl.item_type='note'",
            params![goal_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(GoalStats {
        todo_count: total,
        completed_count: done,
        progress: if total == 0 { 0.0 } else { done as f64 / total as f64 },
        memo_count,
        note_count,
    })
}

/// 删除三级实体时清理其 goal_links/item_tags 关联（防悬挂）。
/// links（wiki 反链）有意保留，失效态由前端显示。
pub fn cleanup_entity_refs(conn: &Connection, item_type: ItemType, item_id: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM goal_links WHERE item_type=?1 AND item_id=?2",
        params![item_type.as_str(), item_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM item_tags WHERE item_type=?1 AND item_id=?2",
        params![item_type.as_str(), item_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- Tauri 命令 ----------

#[tauri::command]
pub fn goal_create(db: State<'_, Db>, input: CreateGoal) -> Result<Goal, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let goal = create_goal(&conn, input)?;
    let _ = crate::services::oplog::record(&conn, "create", "goal", Some(&goal.id), &goal.title);
    Ok(goal)
}

#[tauri::command]
pub fn goal_update(db: State<'_, Db>, id: String, input: UpdateGoal) -> Result<Goal, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let goal = update_goal(&conn, &id, input)?;
    let _ = crate::services::oplog::record(&conn, "update", "goal", Some(&goal.id), &goal.title);
    Ok(goal)
}

#[tauri::command]
pub fn goal_delete(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let title = get_goal(&conn, &id).map(|g| g.title).unwrap_or_else(|_| id.clone());
    archive_goal(&conn, &id)?;
    let _ = crate::services::oplog::record(&conn, "delete", "goal", Some(&id), &title);
    Ok(())
}

#[tauri::command]
pub fn goal_list(db: State<'_, Db>, include_archived: Option<bool>) -> Result<Vec<Goal>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_goals(&conn, include_archived.unwrap_or(false))
}

#[tauri::command]
pub fn goal_set_links(
    db: State<'_, Db>,
    goal_id: String,
    items: Vec<GoalLinkInput>,
) -> Result<Vec<GoalLink>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let title = get_goal(&conn, &goal_id).map(|g| g.title).unwrap_or_else(|_| goal_id.clone());
    let links = set_goal_links(&conn, &goal_id, &items)?;
    let _ = crate::services::oplog::record(
        &conn,
        "update",
        "goal",
        Some(&goal_id),
        &format!("{title}：设置关联 {} 项", links.len()),
    );
    Ok(links)
}

#[tauri::command]
pub fn goal_get_links(db: State<'_, Db>, goal_id: String) -> Result<Vec<GoalLink>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    get_goal_links(&conn, &goal_id)
}

#[tauri::command]
pub fn goal_get_stats(db: State<'_, Db>, goal_id: String) -> Result<GoalStats, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    goal_stats(&conn, &goal_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::todo::{create_todo, delete_todo, set_completed};
    use crate::models::{CreateTodo, ItemType};

    fn temp_db(tag: &str) -> (Connection, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-goal-test-{}-{}-{}",
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

    fn goal(conn: &Connection, title: &str) -> Goal {
        create_goal(conn, CreateGoal {
            title: title.into(),
            ..Default::default()
        })
        .unwrap()
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

    /// TASKS 验收：目标关联 4 待办完成 1 → 25%；全完成 → 100%
    #[test]
    fn goal_progress_25_and_100() {
        let (conn, root) = temp_db("progress");
        let conn = &conn;
        let g = goal(conn, "提升技术能力");
        let todos: Vec<_> = ["t1", "t2", "t3", "t4"]
            .iter()
            .map(|t| todo(conn, t))
            .collect();
        let items: Vec<GoalLinkInput> = todos
            .iter()
            .map(|t| GoalLinkInput { item_type: ItemType::Todo, item_id: t.id.clone() })
            .collect();
        let links = set_goal_links(conn, &g.id, &items).unwrap();
        assert_eq!(links.len(), 4);

        set_completed(conn, &todos[0].id, true).unwrap();
        let s = goal_stats(conn, &g.id).unwrap();
        assert_eq!((s.todo_count, s.completed_count), (4, 1));
        assert!((s.progress - 0.25).abs() < 1e-9, "1/4 应为 25%，实际 {}", s.progress);

        for t in &todos[1..] {
            set_completed(conn, &t.id, true).unwrap();
        }
        let s = goal_stats(conn, &g.id).unwrap();
        assert!((s.progress - 1.0).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(root);
    }

    /// F32 读模型回归：get_goal / list_goals 携带关联待办完成率（linkProgress），独立于手动进度
    #[test]
    fn goal_read_model_carries_link_rate() {
        let (conn, root) = temp_db("readmodel");
        let conn = &conn;
        let g = create_goal(conn, CreateGoal {
            title: "读模型目标".into(),
            progress: Some(70), // 手动值，应不影响 linkProgress
            ..Default::default()
        })
        .unwrap();
        let a = todo(conn, "甲");
        let b = todo(conn, "乙");
        let items = vec![
            GoalLinkInput { item_type: ItemType::Todo, item_id: a.id.clone() },
            GoalLinkInput { item_type: ItemType::Todo, item_id: b.id.clone() },
        ];
        set_goal_links(conn, &g.id, &items).unwrap();
        set_completed(conn, &a.id, true).unwrap();

        let one = get_goal(conn, &g.id).unwrap();
        assert_eq!((one.todo_count, one.done_count), (2, 1));
        assert!((one.link_progress - 0.5).abs() < 1e-9, "完成率应 0.5，实际 {}", one.link_progress);
        assert_eq!(one.progress, 70, "手动进度字段仍独立保留");

        let listed = list_goals(conn, false).unwrap();
        let row = listed.iter().find(|x| x.id == g.id).unwrap();
        assert_eq!((row.todo_count, row.done_count), (2, 1));
        assert!((row.link_progress - 0.5).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 一个目标同时关联待办+备忘+笔记，列表不重不漏
    #[test]
    fn mixed_item_types_dedup() {
        let (conn, root) = temp_db("mixed");
        let conn = &conn;
        conn.execute(
            "INSERT INTO memos (id,title,content,created_at,updated_at,archived) VALUES ('m1','备忘甲',NULL,1,1,0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO notes (id,title,file_path,created_at,updated_at,archived) VALUES ('n1','笔记乙','notes/n1.md',1,1,0)",
            [],
        )
        .unwrap();
        let t = todo(conn, "待办丙");
        let g = goal(conn, "混合目标");

        // 重复项 + 空集校验
        let items = vec![
            GoalLinkInput { item_type: ItemType::Todo, item_id: t.id.clone() },
            GoalLinkInput { item_type: ItemType::Memo, item_id: "m1".into() },
            GoalLinkInput { item_type: ItemType::Memo, item_id: "m1".into() }, // 重复
            GoalLinkInput { item_type: ItemType::Note, item_id: "n1".into() },
        ];
        let links = set_goal_links(conn, &g.id, &items).unwrap();
        assert_eq!(links.len(), 3, "重复项应被忽略");
        // 排序无关（DB 按字典序返回），只验"不重不漏"
        assert_eq!(
            links.iter().map(|l| l.item_type.as_str()).collect::<std::collections::BTreeSet<_>>(),
            ["todo", "memo", "note"].into_iter().collect()
        );
        let s = goal_stats(conn, &g.id).unwrap();
        assert_eq!((s.todo_count, s.memo_count, s.note_count), (1, 1, 1));

        // 挂不存在的实体被拒
        let bad = vec![GoalLinkInput { item_type: ItemType::Todo, item_id: "ghost".into() }];
        assert!(set_goal_links(conn, &g.id, &bad).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    /// TASKS 验收：删除关联实体后 goal_links 不残留悬挂
    #[test]
    fn delete_entity_cleans_goal_links() {
        let (conn, root) = temp_db("cleanup");
        let conn = &conn;
        let g = goal(conn, "目标");
        let t = todo(conn, "将删除");
        let items = vec![GoalLinkInput { item_type: ItemType::Todo, item_id: t.id.clone() }];
        set_goal_links(conn, &g.id, &items).unwrap();
        // 同时给待办挂个标签，验证 item_tags 一并清
        crate::commands::tag::set_item_tags(conn, ItemType::Todo, &t.id, &["某标签".to_string()]).unwrap();

        delete_todo(conn, &t.id).unwrap();

        let links = get_goal_links(conn, &g.id).unwrap();
        assert!(links.is_empty(), "goal_links 应随实体删除清空，实际 {links:?}");
        let s = goal_stats(conn, &g.id).unwrap();
        assert_eq!(s.todo_count, 0);
        let tag_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM item_tags WHERE item_id=?1", params![t.id], |r| r.get(0))
            .unwrap();
        assert_eq!(tag_count, 0, "item_tags 也应清理");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn goal_validation_and_archive() {
        let (conn, root) = temp_db("valid");
        let conn = &conn;
        assert!(create_goal(conn, CreateGoal { title: "  ".into(), ..Default::default() }).is_err());
        let g = goal(conn, "目标甲");
        assert!(update_goal(conn, &g.id, UpdateGoal { status: Some("bogus".into()), ..Default::default() }).is_err());

        archive_goal(conn, &g.id).unwrap();
        assert!(list_goals(conn, false).unwrap().iter().all(|x| x.id != g.id));
        assert!(list_goals(conn, true).unwrap().iter().any(|x| x.id == g.id && x.status == "archived"));
        // 归档目标不能改关联
        let t = todo(conn, "t");
        let items = vec![GoalLinkInput { item_type: ItemType::Todo, item_id: t.id.clone() }];
        assert!(set_goal_links(conn, &g.id, &items).is_err(), "归档目标不能再设置关联");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 0005：手动进度（0-100 钳制）与目标日期独立于关联待办完成率
    #[test]
    fn manual_progress_and_target_date() {
        let (conn, root) = temp_db("manual");
        let conn = &conn;
        let g = create_goal(conn, CreateGoal {
            title: "手动进度目标".into(),
            progress: Some(30),
            target_date: Some(1735660800000), // 2024-12-31 UTC 零点
            ..Default::default()
        })
        .unwrap();
        assert_eq!((g.progress, g.target_date), (30, Some(1735660800000)));

        // 关联待办全完成，Goal.progress 不受影响（两套进度独立）
        let t = todo(conn, "待办");
        let items = vec![GoalLinkInput { item_type: ItemType::Todo, item_id: t.id.clone() }];
        set_goal_links(conn, &g.id, &items).unwrap();
        set_completed(conn, &t.id, true).unwrap();
        let s = goal_stats(conn, &g.id).unwrap();
        assert!((s.progress - 1.0).abs() < 1e-9, "关联完成率应为 100%");
        let fresh = get_goal(conn, &g.id).unwrap();
        assert_eq!(fresh.progress, 30, "手动进度不受关联待办影响");

        // 越界钳制 + 清空目标日期
        let up = update_goal(conn, &g.id, UpdateGoal {
            progress: Some(150),
            target_date: Some(None),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(up.progress, 100, "150 应钳制到 100");
        assert_eq!(up.target_date, None, "target_date 应可清空");
        let _ = std::fs::remove_dir_all(root);
    }
}
