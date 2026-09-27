//! Todo 待办命令（F1/F2）：create/update/delete/set_completed/list/get。
//!
//! 分层：普通 repo 函数接 `&Connection`（可直接单测），`#[tauri::command]` 只做加锁薄封装。
//! completed_at/updated_at/id 一律后端写；删除的二次确认在前端。

use rusqlite::{params, types::Value, Connection, OptionalExtension, Row};
use tauri::State;

use crate::db::Db;
use crate::models::{
    COLORS, CATEGORIES, PRIORITIES, BatchTodoPatch, CreateTodo, Todo, TodoFilter, UpdateTodo,
};

const COLUMNS: &str = "id, title, content, completed, date, sort_order, color, category, priority, \
     project_id, time, parent_id, postpone_auto, created_at, updated_at, completed_at";

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

fn map_row(row: &Row<'_>) -> rusqlite::Result<Todo> {
    Ok(Todo {
        id: row.get("id")?,
        title: row.get("title")?,
        content: row.get("content")?,
        completed: row.get::<_, i64>("completed")? != 0,
        date: row.get("date")?,
        sort_order: row.get("sort_order")?,
        color: row.get("color")?,
        category: row.get("category")?,
        priority: row.get("priority")?,
        project_id: row.get("project_id")?,
        time: row.get("time")?,
        parent_id: row.get("parent_id")?,
        postpone_auto: row.get::<_, i64>("postpone_auto")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        completed_at: row.get("completed_at")?,
        tags: Vec::new(),
    })
}

/// 批量填充一组 todo 的标签名（一次查询，避免 N+1）。
fn fill_tags(conn: &Connection, todos: &mut [Todo]) -> Result<(), String> {
    if todos.is_empty() {
        return Ok(());
    }
    let ids: Vec<Value> = todos.iter().map(|t| Value::Text(t.id.clone())).collect();
    let placeholders = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT it.item_id, t.name FROM item_tags it JOIN tags t ON t.id = it.tag_id
         WHERE it.item_type='todo' AND it.item_id IN ({placeholders}) ORDER BY t.name"
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
    for t in todos.iter_mut() {
        if let Some(names) = map.remove(&t.id) {
            t.tags = names;
        }
    }
    Ok(())
}

fn validate_date(d: Option<i64>) -> Result<(), String> {
    if let Some(d) = d {
        if !(10_000_000..=99_999_999).contains(&d) {
            return Err(format!("date 必须是 8 位 YYYYMMDD，实际 {d}"));
        }
        // 校验真实年月日（拒绝 20261340 这类 8 位但月日非法的值）
        let (y, m, day) = (d / 10_000, (d / 100) % 100, d % 100);
        chrono::NaiveDate::from_ymd_opt(y as i32, m as u32, day as u32)
            .ok_or_else(|| format!("date 不是有效日期（YYYYMMDD）：{d}"))?;
    }
    Ok(())
}

fn validate_time(t: &Option<String>) -> Result<(), String> {
    if let Some(t) = t {
        let b = t.as_bytes();
        let ok = b.len() == 5
            && b[2] == b':'
            && t[0..2].parse::<u8>().is_ok_and(|h| h <= 23)
            && t[3..5].parse::<u8>().is_ok_and(|m| m <= 59);
        if !ok {
            return Err(format!("time 必须是 HH:MM，实际 {t}"));
        }
    }
    Ok(())
}

fn validate_enums(category: &str, priority: &str, color: &Option<String>) -> Result<(), String> {
    if !CATEGORIES.contains(&category) {
        return Err(format!("category 必须是 {CATEGORIES:?}，实际 {category}"));
    }
    if !PRIORITIES.contains(&priority) {
        return Err(format!("priority 必须是 {PRIORITIES:?}，实际 {priority}"));
    }
    if let Some(c) = color {
        if !COLORS.contains(&c.as_str()) {
            return Err(format!("color 必须是 {COLORS:?}，实际 {c}"));
        }
    }
    Ok(())
}

// ---------- repo 层 ----------

pub fn create_todo(conn: &Connection, input: CreateTodo) -> Result<Todo, String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err("标题不能为空".into());
    }
    let category = input.category.unwrap_or_else(|| "life".to_string());
    let priority = input.priority.unwrap_or_else(|| "medium".to_string());
    validate_enums(&category, &priority, &input.color)?;
    validate_date(input.date)?;
    validate_time(&input.time)?;

    let id = crate::id::new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO todos
           (id,title,content,completed,date,sort_order,color,category,priority,
            project_id,time,parent_id,postpone_auto,created_at,updated_at,completed_at)
         VALUES (?1,?2,?3,0,?4,?5,?6,?7,?8,?9,?10,?11,0,?12,?12,NULL)",
        params![
            id,
            title,
            input.content,
            input.date,
            input.sort_order.unwrap_or(0),
            input.color,
            category,
            priority,
            input.project_id,
            input.time,
            input.parent_id,
            now,
        ],
    )
    .map_err(|e| format!("创建待办失败：{e}"))?;

    // T4.5：同步 FTS 索引
    crate::services::search::sync_todo_fts(conn, &id)?;
    get_todo(conn, &id)?.ok_or_else(|| "创建后查无此待办".to_string())
}

pub fn get_todo(conn: &Connection, id: &str) -> Result<Option<Todo>, String> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM todos WHERE id = ?1"),
        params![id],
        map_row,
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn update_todo(conn: &Connection, id: &str, input: UpdateTodo) -> Result<Todo, String> {
    if get_todo(conn, id)?.is_none() {
        return Err(format!("待办不存在：{id}"));
    }

    // 逐字段校验
    if let Some(t) = &input.title {
        if t.trim().is_empty() {
            return Err("标题不能为空".into());
        }
    }
    if let Some(c) = &input.category {
        if !CATEGORIES.contains(&c.as_str()) {
            return Err(format!("category 必须是 {CATEGORIES:?}，实际 {c}"));
        }
    }
    if let Some(p) = &input.priority {
        if !PRIORITIES.contains(&p.as_str()) {
            return Err(format!("priority 必须是 {PRIORITIES:?}，实际 {p}"));
        }
    }
    if let Some(Some(c)) = &input.color {
        if !COLORS.contains(&c.as_str()) {
            return Err(format!("color 必须是 {COLORS:?}，实际 {c}"));
        }
    }
    if let Some(Some(d)) = input.date {
        validate_date(Some(d))?;
    }
    if let Some(Some(t)) = &input.time {
        validate_time(&Some(t.clone()))?;
    }

    // 动态拼 SET（completed/completed_at 不在此改）
    let mut sets: Vec<String> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    let mut push = |col: &str, v: Value| {
        sets.push(format!("{col} = ?{}", vals.len() + 1));
        vals.push(v);
    };

    if let Some(v) = input.title {
        push("title", Value::Text(v.trim().to_string()));
    }
    if let Some(v) = input.content {
        push("content", v.map_or(Value::Null, Value::Text));
    }
    if let Some(v) = input.date {
        push("date", v.map_or(Value::Null, Value::Integer));
    }
    if let Some(v) = input.color {
        push("color", v.map_or(Value::Null, Value::Text));
    }
    if let Some(v) = input.category {
        push("category", Value::Text(v));
    }
    if let Some(v) = input.priority {
        push("priority", Value::Text(v));
    }
    if let Some(v) = input.project_id {
        push("project_id", v.map_or(Value::Null, Value::Text));
    }
    if let Some(v) = input.time {
        push("time", v.map_or(Value::Null, Value::Text));
    }
    if let Some(v) = input.parent_id {
        push("parent_id", v.map_or(Value::Null, Value::Text));
    }
    if let Some(v) = input.sort_order {
        push("sort_order", Value::Integer(v));
    }

    if sets.is_empty() {
        return get_todo(conn, id)?.ok_or_else(|| "待办不存在".to_string());
    }
    sets.push(format!("updated_at = ?{}", vals.len() + 1));
    vals.push(Value::Integer(now_ms()));
    vals.push(Value::Text(id.to_string()));

    let sql = format!("UPDATE todos SET {} WHERE id = ?{}", sets.join(", "), vals.len());
    let affected = conn
        .execute(&sql, rusqlite::params_from_iter(vals))
        .map_err(|e| format!("更新待办失败：{e}"))?;
    if affected == 0 {
        return Err(format!("待办不存在：{id}"));
    }
    crate::services::search::sync_todo_fts(conn, id)?;
    get_todo(conn, id)?.ok_or_else(|| "更新后查无此待办".to_string())
}

pub fn set_completed(conn: &Connection, id: &str, completed: bool) -> Result<Todo, String> {
    // 重复实例：id 命中 todo_repeat_items → 完成标记落实例行，不动模板。
    if crate::services::repeat::is_repeat_instance(conn, id)? {
        crate::services::repeat::set_instance_completed(conn, id, completed)?;
        return crate::services::repeat::get_instance_todo(conn, id)?
            .ok_or_else(|| "更新后查无此实例".to_string());
    }
    if get_todo(conn, id)?.is_none() {
        return Err(format!("待办不存在：{id}"));
    }
    let now = now_ms();
    let completed_at: Option<i64> = if completed { Some(now) } else { None };
    conn.execute(
        "UPDATE todos SET completed = ?2, completed_at = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, completed as i64, completed_at, now],
    )
    .map_err(|e| e.to_string())?;
    get_todo(conn, id)?.ok_or_else(|| "更新后查无此待办".to_string())
}

pub fn delete_todo(conn: &Connection, id: &str) -> Result<(), String> {
    // 事务：删待办（todo_assignees 靠 CASCADE）+ 清 goal_links/item_tags 悬挂
    //（links 有意保留作失效态，SPEC T2.4）
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let n = tx
        .execute("DELETE FROM todos WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("待办不存在：{id}"));
    }
    crate::commands::goal::cleanup_entity_refs(&tx, crate::models::ItemType::Todo, id)?;
    tx.commit().map_err(|e| e.to_string())?;
    crate::services::search::sync_todo_fts(conn, id)
}

/// 批量更新（多选批量操作）：单事务按 patch 拼 UPDATE（updated_at=now），
/// 只更新 patch 里出现的字段；commit 后逐个同步 FTS。返回实际更新条数。
pub fn batch_update_todos(conn: &Connection, ids: &[String], patch: &BatchTodoPatch) -> Result<i64, String> {
    if ids.is_empty() {
        return Err("ids 不能为空".into());
    }
    // 逐字段校验（照抄 update_todo 的逻辑）
    if let Some(p) = &patch.priority {
        if !PRIORITIES.contains(&p.as_str()) {
            return Err(format!("priority 必须是 {PRIORITIES:?}，实际 {p}"));
        }
    }
    if let Some(c) = &patch.category {
        if !CATEGORIES.contains(&c.as_str()) {
            return Err(format!("category 必须是 {CATEGORIES:?}，实际 {c}"));
        }
    }
    if let Some(Some(c)) = &patch.color {
        if !COLORS.contains(&c.as_str()) {
            return Err(format!("color 必须是 {COLORS:?}，实际 {c}"));
        }
    }
    if let Some(Some(d)) = patch.date {
        validate_date(Some(d))?;
    }
    if let Some(Some(pid)) = &patch.project_id {
        let exists: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects WHERE id=?1", params![pid], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if exists == 0 {
            return Err(format!("项目不存在：{pid}"));
        }
    }

    let now = now_ms();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    // 生成 "start、start+1、…" 形态的 IN 占位符（start 从 1 计，参数逐个绑定）
    let in_placeholders = |start: usize| {
        ids.iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", start + i))
            .collect::<Vec<_>>()
            .join(", ")
    };

    // completed 语义（对齐 dashboard.rs 的 batch_complete_date：先分配标记，后整体；
    // 完成/取消都不触发转备忘）
    if patch.completed.is_some() {
        let done = patch.completed.unwrap();
        if done {
            let sql = format!(
                "UPDATE todo_assignees SET completed=1, completed_at=COALESCE(completed_at,?1)
                 WHERE todo_id IN ({})",
                in_placeholders(2)
            );
            let mut vals = vec![Value::Integer(now)];
            vals.extend(ids.iter().map(|id| Value::Text(id.clone())));
            tx.execute(&sql, rusqlite::params_from_iter(&vals))
                .map_err(|e| e.to_string())?;
        } else {
            let sql = format!(
                "UPDATE todo_assignees SET completed=0, completed_at=NULL WHERE todo_id IN ({})",
                in_placeholders(1)
            );
            let vals: Vec<Value> = ids.iter().map(|id| Value::Text(id.clone())).collect();
            tx.execute(&sql, rusqlite::params_from_iter(&vals))
                .map_err(|e| e.to_string())?;
        }
    }

    // 动态拼 todos 的 SET
    let mut sets: Vec<String> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    let mut push = |col: &str, v: Value| {
        sets.push(format!("{col} = ?{}", vals.len() + 1));
        vals.push(v);
    };
    if let Some(v) = patch.date {
        push("date", v.map_or(Value::Null, Value::Integer));
    }
    if let Some(v) = &patch.color {
        push("color", v.clone().map_or(Value::Null, Value::Text));
    }
    if let Some(v) = &patch.category {
        push("category", Value::Text(v.clone()));
    }
    if let Some(v) = &patch.priority {
        push("priority", Value::Text(v.clone()));
    }
    if let Some(v) = &patch.project_id {
        push("project_id", v.clone().map_or(Value::Null, Value::Text));
    }
    match patch.completed {
        Some(true) => {
            push("completed", Value::Integer(1));
            sets.push(format!("completed_at = COALESCE(completed_at, ?{})", vals.len() + 1));
            vals.push(Value::Integer(now));
        }
        Some(false) => {
            push("completed", Value::Integer(0));
            sets.push("completed_at = NULL".into());
        }
        None => {}
    }
    sets.push(format!("updated_at = ?{}", vals.len() + 1));
    vals.push(Value::Integer(now));

    let start = vals.len() + 1;
    vals.extend(ids.iter().map(|id| Value::Text(id.clone())));
    let sql = format!(
        "UPDATE todos SET {} WHERE id IN ({})",
        sets.join(", "),
        in_placeholders(start)
    );
    let n = tx
        .execute(&sql, rusqlite::params_from_iter(&vals))
        .map_err(|e| format!("批量更新失败：{e}"))?;
    tx.commit().map_err(|e| e.to_string())?;

    for id in ids {
        crate::services::search::sync_todo_fts(conn, id)?;
    }
    Ok(n as i64)
}

/// 批量删除（多选批量操作）：单事务 DELETE（todo_assignees 靠 CASCADE）+
/// 清 goal_links/item_tags 悬挂；links 有意保留作失效态（与 delete_todo 一致）。
/// commit 后逐个同步 FTS。返回删除条数（不存在的 id 不计数）。
pub fn batch_delete_todos(conn: &Connection, ids: &[String]) -> Result<i64, String> {
    if ids.is_empty() {
        return Err("ids 不能为空".into());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let placeholders = ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let vals: Vec<Value> = ids.iter().map(|id| Value::Text(id.clone())).collect();
    let n = tx
        .execute(
            &format!("DELETE FROM todos WHERE id IN ({placeholders})"),
            rusqlite::params_from_iter(&vals),
        )
        .map_err(|e| format!("批量删除失败：{e}"))?;
    for id in ids {
        crate::commands::goal::cleanup_entity_refs(&tx, crate::models::ItemType::Todo, id)?;
    }
    tx.commit().map_err(|e| e.to_string())?;

    for id in ids {
        crate::services::search::sync_todo_fts(conn, id)?;
    }
    Ok(n as i64)
}

/// 同分区（如今日未完成）拖拽排序：按给定 id 顺序写 sort_order，单事务。
pub fn reorder_todos(conn: &Connection, ordered_ids: &[String]) -> Result<(), String> {
    let now = now_ms();
    // unchecked_transaction 接 &self（repo 层统一 &Connection）；BEGIN/COMMIT 由 rusqlite 管理。
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare("UPDATE todos SET sort_order = ?2, updated_at = ?3 WHERE id = ?1")
            .map_err(|e| e.to_string())?;
        for (i, id) in ordered_ids.iter().enumerate() {
            stmt.execute(params![id, i as i64, now])
                .map_err(|e| format!("排序失败（{id}）：{e}"))?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn list_todos(conn: &Connection, filter: &TodoFilter) -> Result<Vec<Todo>, String> {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    // 追加 "col = ?N" 条件并记录参数
    macro_rules! bind_eq {
        ($col:expr, $v:expr) => {{
            where_clauses.push(format!("{} = ?{}", $col, vals.len() + 1));
            vals.push($v);
        }};
    }

    if let Some(d) = filter.date {
        bind_eq!("date", Value::Integer(d));
    }
    if filter.inbox == Some(true) {
        where_clauses.push("date IS NULL".into());
    }
    if let Some(c) = filter.completed {
        bind_eq!("completed", Value::Integer(c as i64));
    }
    if let Some(cat) = &filter.category {
        bind_eq!("category", Value::Text(cat.clone()));
    }
    if let Some(p) = &filter.project_id {
        bind_eq!("project_id", Value::Text(p.clone()));
    }
    if let Some(person) = &filter.assignee {
        where_clauses.push(format!(
            "EXISTS (SELECT 1 FROM todo_assignees ta WHERE ta.todo_id = todos.id AND ta.person_id = ?{})",
            vals.len() + 1
        ));
        vals.push(Value::Text(person.clone()));
    }
    if let Some(p) = &filter.priority {
        bind_eq!("priority", Value::Text(p.clone()));
    }
    if let Some(tag_name) = &filter.tag {
        let n = vals.len() + 1;
        where_clauses.push(format!(
            "EXISTS (SELECT 1 FROM item_tags it JOIN tags t ON t.id = it.tag_id
                     WHERE it.item_type='todo' AND it.item_id = todos.id AND t.name = ?{n})"
        ));
        vals.push(Value::Text(tag_name.clone()));
    }
    if let Some(keyword) = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        // 标题 / 描述 / 标签名 三处 LIKE，同一关键词绑定 3 次
        let n = vals.len() + 1;
        where_clauses.push(format!(
            "(title LIKE ?{n} OR content LIKE ?{n1} OR EXISTS (
                SELECT 1 FROM item_tags it JOIN tags t ON t.id = it.tag_id
                WHERE it.item_type='todo' AND it.item_id = todos.id AND t.name LIKE ?{n2}
            ))",
            n1 = n + 1,
            n2 = n + 2,
        ));
        let like = Value::Text(format!("%{keyword}%"));
        vals.push(like.clone());
        vals.push(like.clone());
        vals.push(like);
    }

    // F15 修复：纯"某日"视图（Today）需把重复实例并入；同时排除模板行，
    // 避免模板在其原始创建日与实例同日重复出现。其它筛选（分类/项目/关键词/收集箱）不受影响。
    let merge_instances = filter.date.is_some()
        && filter.inbox != Some(true)
        && filter.category.is_none()
        && filter.priority.is_none()
        && filter.project_id.is_none()
        && filter.assignee.is_none()
        && filter.tag.is_none()
        && filter.q.as_deref().map(str::trim).unwrap_or("").is_empty();
    if merge_instances {
        where_clauses.push("id NOT IN (SELECT todo_id FROM todo_repeat_rules)".into());
        if let Some(d) = filter.date {
            crate::services::repeat::ensure_instances_for_date(conn, d)?;
        }
    }

    // 排序：默认「未完成置顶 + sort_order（拖拽）+ 创建时间」；
    // filter.sort == "priority" 时在同组内优先按优先级 高→中→低→无。
    let order_by = if filter.sort.as_deref() == Some("priority") {
        "ORDER BY completed ASC, \
         CASE priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 WHEN 'low' THEN 2 ELSE 3 END ASC, \
         sort_order ASC, created_at ASC, id ASC"
    } else {
        "ORDER BY completed ASC, sort_order ASC, created_at ASC, id ASC"
    };
    let sql = format!(
        // 未完成置顶、完成沉底；同组内按 sort_order（拖拽）、创建时间
        "SELECT {COLUMNS} FROM todos {} {}",
        if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        },
        order_by
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(vals), map_row)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    fill_tags(conn, &mut out)?;
    if merge_instances {
        if let Some(d) = filter.date {
            let mut inst = crate::services::repeat::instances_for_date(conn, d, filter.completed)?;
            out.append(&mut inst);
            // 复刻后端排序：未完成置顶、完成沉底；组内 sort_order → created_at → id。
            out.sort_by(|a, b| {
                (a.completed, a.sort_order, a.created_at, a.id.as_str()).cmp(&(
                    b.completed,
                    b.sort_order,
                    b.created_at,
                    b.id.as_str(),
                ))
            });
        }
    }
    Ok(out)
}

/// 连带完成（T3.1 产品决策的批量入口）：勾选父项 + 全部后代（递归收集）。
/// 正常勾选父项不自动勾子项（父/子独立），此命令供用户显式"连带完成"。
pub fn complete_with_children(conn: &Connection, id: &str) -> Result<usize, String> {
    if get_todo(conn, id)?.is_none() {
        return Err(format!("待办不存在：{id}"));
    }
    // 递归收集后代（CEF 递归 CTE）
    let mut ids: Vec<String> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "WITH RECURSIVE desc(oid) AS (
                   SELECT id FROM todos WHERE parent_id = ?1
                   UNION ALL
                   SELECT t.id FROM todos t JOIN desc d ON t.parent_id = d.oid
                 )
                 SELECT oid FROM desc",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![id], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for r in rows {
            ids.push(r.map_err(|e| e.to_string())?);
        }
    }

    let now = now_ms();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut done = 1; // 父自身
    tx.execute(
        "UPDATE todos SET completed=1, completed_at=COALESCE(completed_at,?2), updated_at=?2 WHERE id=?1",
        params![id, now],
    )
    .map_err(|e| e.to_string())?;
    for cid in &ids {
        // 若子任务有分配人（多人协作），先把所有人的分配标记置完成，
        // 再置整体 completed——与 person_toggle 的 recompute 语义一致（对抗复核②）。
        tx.execute(
            "UPDATE todo_assignees SET completed=1,
                    completed_at=COALESCE(completed_at,?2)
             WHERE todo_id=?1 AND completed=0",
            params![cid, now],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE todos SET completed=1, completed_at=COALESCE(completed_at,?2), updated_at=?2 WHERE id=?1",
            params![cid, now],
        )
        .map_err(|e| e.to_string())?;
        done += 1;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(done)
}

// ---------- Tauri 命令薄封装 ----------

#[tauri::command]
pub fn todo_create(db: State<'_, Db>, input: CreateTodo) -> Result<Todo, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    create_todo(&conn, input)
}

#[tauri::command]
pub fn todo_get(db: State<'_, Db>, id: String) -> Result<Option<Todo>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut todo = get_todo(&conn, &id)?;
    if let Some(t) = todo.as_mut() {
        fill_tags(&conn, std::slice::from_mut(t))?;
    }
    Ok(todo)
}

#[tauri::command]
pub fn todo_update(db: State<'_, Db>, id: String, input: UpdateTodo) -> Result<Todo, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    update_todo(&conn, &id, input)
}

#[tauri::command]
pub fn todo_delete(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    delete_todo(&conn, &id)
}

#[tauri::command]
pub fn todo_set_completed(db: State<'_, Db>, id: String, completed: bool) -> Result<Todo, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    set_completed(&conn, &id, completed)
}

#[tauri::command]
pub fn todo_list(db: State<'_, Db>, filter: Option<TodoFilter>) -> Result<Vec<Todo>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_todos(&conn, filter.as_ref().unwrap_or(&TodoFilter::default()))
}

#[tauri::command]
pub fn todo_reorder(db: State<'_, Db>, ordered_ids: Vec<String>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    reorder_todos(&conn, &ordered_ids)
}

/// 连带完成：父 + 全部后代（T3.1 批量入口）。
#[tauri::command]
pub fn todo_complete_with_children(db: State<'_, Db>, id: String) -> Result<usize, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    complete_with_children(&conn, &id)
}

/// 批量创建（F36/T3.9 剪贴板导入）：分批（每 200 条一个事务）入库。
#[tauri::command]
#[allow(clippy::type_complexity)]
pub fn todos_bulk_create(
    db: State<'_, Db>,
    items: Vec<CreateTodo>,
) -> Result<(usize, Vec<String>), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut created = 0usize;
    let mut failed: Vec<String> = Vec::new();
    // 分批：每 200 条一个事务（SPEC §13.3，中途失败回滚该批）
    for chunk in items.chunks(200) {
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut batch_failed = 0usize;
        for item in chunk {
            match create_todo(&tx, item.clone()) {
                Ok(_) => created += 1,
                Err(e) => {
                    batch_failed += 1;
                    if failed.len() < 50 {
                        failed.push(format!("{}（{e}）", item.title));
                    }
                }
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        let _ = batch_failed;
    }
    Ok((created, failed))
}

/// 批量更新（多选批量操作）：单事务 + FTS 同步，返回实际更新条数。
#[tauri::command]
pub fn todo_batch_update(
    db: State<'_, Db>,
    ids: Vec<String>,
    patch: BatchTodoPatch,
) -> Result<i64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    batch_update_todos(&conn, &ids, &patch)
}

/// 批量删除（多选批量操作）：单事务 + FTS 同步，返回删除条数。
#[tauri::command]
pub fn todo_batch_delete(db: State<'_, Db>, ids: Vec<String>) -> Result<i64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    batch_delete_todos(&conn, &ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_at;
    use std::path::PathBuf;

    fn temp_db(tag: &str) -> (Connection, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-todo-test-{}-{}-{}",
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

    fn mk(title: &str) -> CreateTodo {
        CreateTodo {
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
        }
    }

    #[test]
    fn create_complete_delete_lifecycle() {
        let (conn, root) = temp_db("life");
        let t = create_todo(&conn, mk("写测试")).unwrap();
        assert_eq!(t.title, "写测试");
        assert!(!t.completed);
        assert_eq!(t.category, "life"); // 默认 life
        assert_eq!(t.priority, "medium"); // 默认 medium
        assert!(t.completed_at.is_none());
        assert_eq!(t.id.len(), 18);

        // 勾选完成 → completed=1 + completed_at 有值
        let done = set_completed(&conn, &t.id, true).unwrap();
        assert!(done.completed);
        assert!(done.completed_at.is_some());
        assert!(done.updated_at >= t.updated_at);

        // 取消完成 → completed_at 清空
        let undone = set_completed(&conn, &t.id, false).unwrap();
        assert!(!undone.completed);
        assert!(undone.completed_at.is_none());

        // 删除
        delete_todo(&conn, &t.id).unwrap();
        assert!(get_todo(&conn, &t.id).unwrap().is_none());
        // 再删报错
        assert!(delete_todo(&conn, &t.id).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[allow(clippy::inconsistent_digit_grouping)] // 日期字面量按 YYYY_MM_DD 分组，可读性优先
    fn rejects_invalid_input() {
        let (conn, root) = temp_db("invalid");
        let mut bad = mk("   ");
        assert!(create_todo(&conn, bad.clone()).is_err(), "空标题必须拒绝");
        bad.title = "x".into();
        bad.category = Some("school".into());
        assert!(create_todo(&conn, bad.clone()).is_err(), "非法 category 拒绝");
        bad.category = None;
        bad.priority = Some("urgent".into());
        assert!(create_todo(&conn, bad.clone()).is_err(), "非法 priority 拒绝");
        bad.priority = None;
        bad.color = Some("pink".into());
        assert!(create_todo(&conn, bad.clone()).is_err(), "非法 color 拒绝");
        bad.color = None;
        bad.date = Some(2026_13_40);
        assert!(create_todo(&conn, bad.clone()).is_err(), "非法 date 拒绝");
        bad.date = Some(2026_02_29);
        assert!(create_todo(&conn, bad.clone()).is_err(), "2026 非闰年，0229 拒绝");
        bad.date = None;
        bad.time = Some("25:00".into());
        assert!(create_todo(&conn, bad).is_err(), "非法 time 拒绝");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn update_partial_and_clear_fields() {
        let (conn, root) = temp_db("update");
        let t = create_todo(&conn, mk("原标题")).unwrap();
        let updated = update_todo(
            &conn,
            &t.id,
            UpdateTodo {
                title: Some("新标题".to_string()),
                category: Some("work".to_string()),
                priority: Some("high".to_string()),
                date: Some(Some(20260916)),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.category, "work");
        assert_eq!(updated.priority, "high");
        assert_eq!(updated.date, Some(20260916));
        assert!(updated.updated_at >= t.updated_at);

        // 显式 null 清空日期 → 回到收集箱
        let cleared = update_todo(
            &conn,
            &t.id,
            UpdateTodo {
                date: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(cleared.date.is_none());

        // 空标题更新被拒
        assert!(update_todo(
            &conn,
            &t.id,
            UpdateTodo {
                title: Some("  ".into()),
                ..Default::default()
            }
        )
        .is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn list_filters_work() {
        let (conn, root) = temp_db("list");
        // 两个项目
        conn.execute(
            "INSERT INTO projects (id,name,description,color,sort_order,archived,created_at,updated_at)
             VALUES ('p1','P1',NULL,NULL,0,0,1,1),('p2','P2',NULL,NULL,0,0,1,1)",
            [],
        )
        .unwrap();
        let mk_todo = |title: &str, category: &str, date: Option<i64>, project: Option<&str>| {
            let mut t = mk(title);
            t.category = Some(category.into());
            t.date = date;
            t.project_id = project.map(String::from);
            create_todo(&conn, t).unwrap()
        };
        let work_today = mk_todo("工作今天", "work", Some(20260916), Some("p1"));
        let _work_other = mk_todo("工作他日", "work", Some(20260917), Some("p2"));
        let _life_inbox = mk_todo("生活收集箱", "life", None, None);
        // 完成工作今天
        set_completed(&conn, &work_today.id, true).unwrap();
        // 一条分配给某人的工作待办（先建 person 满足 FK，再建分配行）
        let assigned = mk_todo("分派任务", "work", Some(20260916), None);
        conn.execute(
            "INSERT INTO people (id,name,note,sort_order,archived,created_at,updated_at) VALUES ('person1','张三',NULL,0,0,1,1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO todo_assignees (todo_id,person_id,completed,completed_at) VALUES (?1,'person1',0,NULL)",
            params![assigned.id],
        )
        .unwrap();

        assert_eq!(list_todos(&conn, &TodoFilter::default()).unwrap().len(), 4);
        assert_eq!(
            list_todos(&conn, &TodoFilter { date: Some(20260916), ..Default::default() })
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            list_todos(&conn, &TodoFilter { inbox: Some(true), ..Default::default() })
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list_todos(&conn, &TodoFilter { completed: Some(true), ..Default::default() })
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list_todos(&conn, &TodoFilter { category: Some("life".into()), ..Default::default() })
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list_todos(&conn, &TodoFilter { project_id: Some("p1".into()), ..Default::default() })
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list_todos(&conn, &TodoFilter { assignee: Some("person1".into()), ..Default::default() })
                .unwrap()
                .len(),
            1
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn completed_sinks_to_bottom() {
        let (conn, root) = temp_db("sink");
        let d = Some(20260916);
        let a = create_todo(&conn, { let mut x = mk("A"); x.date = d; x }).unwrap();
        let b = create_todo(&conn, { let mut x = mk("B"); x.date = d; x }).unwrap();
        let c = create_todo(&conn, { let mut x = mk("C"); x.date = d; x }).unwrap();
        // 完成 B
        set_completed(&conn, &b.id, true).unwrap();
        let list = list_todos(&conn, &TodoFilter { date: Some(20260916), ..Default::default() }).unwrap();
        let order: Vec<&str> = list.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(order, vec!["A", "C", "B"], "完成项 B 应沉底，实际 {order:?}");
        assert!(!list[0].completed && !list[1].completed && list[2].completed);
        // 取消完成回到未完成组（sort_order 都是 0，按 created_at，仍在前两位之一）
        set_completed(&conn, &b.id, false).unwrap();
        let list2 = list_todos(&conn, &TodoFilter { date: Some(20260916), ..Default::default() }).unwrap();
        assert!(list2.iter().all(|t| !t.completed));
        let _ = (a, c);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn priority_filter_and_keyword_search() {
        let (conn, root) = temp_db("search");
        let mut hi = mk("写周报");
        hi.priority = Some("high".into());
        hi.content = Some("包含季度总结关键词".into());
        let hi = create_todo(&conn, hi).unwrap();
        let mut tagged = mk("普通任务");
        tagged.priority = Some("low".into());
        let tagged = create_todo(&conn, tagged).unwrap();
        // 给 tagged 挂标签"会议"
        conn.execute("INSERT INTO tags (id,name) VALUES ('t1','会议')", []).unwrap();
        conn.execute(
            "INSERT INTO item_tags (tag_id,item_type,item_id) VALUES ('t1','todo',?1)",
            params![tagged.id],
        )
        .unwrap();
        create_todo(&conn, mk("另一件事")).unwrap();

        // priority 过滤
        assert_eq!(
            list_todos(&conn, &TodoFilter { priority: Some("high".into()), ..Default::default() })
                .unwrap()
                .len(),
            1
        );
        // q 命中标题
        assert_eq!(
            list_todos(&conn, &TodoFilter { q: Some("周报".into()), ..Default::default() })
                .unwrap()[0]
                .id,
            hi.id
        );
        // q 命中描述
        assert_eq!(
            list_todos(&conn, &TodoFilter { q: Some("季度".into()), ..Default::default() })
                .unwrap()
                .len(),
            1
        );
        // q 命中标签名
        assert_eq!(
            list_todos(&conn, &TodoFilter { q: Some("会议".into()), ..Default::default() })
                .unwrap()[0]
                .id,
            tagged.id
        );
        // 组合：category + q（无工作类含"会议"）→ 0
        assert_eq!(
            list_todos(
                &conn,
                &TodoFilter { category: Some("work".into()), q: Some("会议".into()), ..Default::default() }
            )
            .unwrap()
            .len(),
            0
        );

        // 精确 tag 过滤 vs q 模糊：新建标题含"会议"但无标签的任务
        create_todo(&conn, mk("又是会议")).unwrap();
        let by_tag = list_todos(&conn, &TodoFilter { tag: Some("会议".into()), ..Default::default() }).unwrap();
        assert_eq!(by_tag.len(), 1, "tag 精确过滤只命中挂了标签的那条");
        assert_eq!(by_tag[0].id, tagged.id);
        assert_eq!(by_tag[0].tags, vec!["会议".to_string()], "list 返回应带标签名");
        let by_q = list_todos(&conn, &TodoFilter { q: Some("会议".into()), ..Default::default() }).unwrap();
        assert!(by_q.len() >= 2, "q 模糊应同时命中标签与标题");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reorder_persists_within_partition() {
        let (conn, root) = temp_db("reorder");
        let d = Some(20260916);
        let a = create_todo(&conn, { let mut x = mk("A"); x.date = d; x }).unwrap();
        let b = create_todo(&conn, { let mut x = mk("B"); x.date = d; x }).unwrap();
        let c = create_todo(&conn, { let mut x = mk("C"); x.date = d; x }).unwrap();
        // 拖成 C,A,B
        reorder_todos(&conn, &[c.id.clone(), a.id.clone(), b.id.clone()]).unwrap();
        let list = list_todos(&conn, &TodoFilter { date: Some(20260916), ..Default::default() }).unwrap();
        assert_eq!(
            list.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            vec!["C", "A", "B"]
        );
        // 新连接（模拟重启）顺序不变
        let path = {
            let mut p = root.clone();
            p.push("todolist.db");
            p
        };
        let conn2 = Connection::open(&path).unwrap();
        let list2 = list_todos(&conn2, &TodoFilter { date: Some(20260916), ..Default::default() }).unwrap();
        assert_eq!(
            list2.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            vec!["C", "A", "B"],
            "拖拽顺序必须持久化，重启不乱"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// T3.1：3 层嵌套创建；父完成不自动勾子（产品决策）；连带完成递归；删除父级联删子
    #[test]
    fn subtasks_nesting_and_cascade() {
        let (conn, root) = temp_db("sub");
        let parent = create_todo(&conn, { let mut x = mk("父任务"); x.date = Some(20260920); x }).unwrap();
        // 二层
        let mut child1 = mk("子任务一");
        child1.parent_id = Some(parent.id.clone());
        child1.date = Some(20260920);
        let child1 = create_todo(&conn, child1).unwrap();
        // 三层（子的子）
        let mut grand = mk("孙任务");
        grand.parent_id = Some(child1.id.clone());
        grand.date = Some(20260920);
        let grand = create_todo(&conn, grand).unwrap();
        let mut child2 = mk("子任务二");
        child2.parent_id = Some(parent.id.clone());
        child2.date = Some(20260920);
        let child2 = create_todo(&conn, child2).unwrap();

        // 父完成 → 子/孙不受影响（产品决策：父子独立）
        set_completed(&conn, &parent.id, true).unwrap();
        for t in [&child1, &child2, &grand] {
            assert!(!get_todo(&conn, &t.id).unwrap().unwrap().completed, "父完成不应波及 {}", t.title);
        }
        set_completed(&conn, &parent.id, false).unwrap();

        // 连带完成：父 + 3 后代全勾
        let done = complete_with_children(&conn, &parent.id).unwrap();
        assert_eq!(done, 4, "父+3 后代应共 4 条");
        for t in [&child1, &child2, &grand] {
            assert!(get_todo(&conn, &t.id).unwrap().unwrap().completed);
        }

        // 删除父 → 子/孙 CASCADE 级联消失（ON DELETE CASCADE 验证）
        delete_todo(&conn, &parent.id).unwrap();
        for t in [&child1, &child2, &grand] {
            assert!(get_todo(&conn, &t.id).unwrap().is_none(), "{} 应随父级联删除", t.title);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn foreign_key_blocks_bad_project() {
        let (conn, root) = temp_db("fk");
        let mut t = mk("挂不存在项目");
        t.project_id = Some("ghost".into());
        let err = create_todo(&conn, t).unwrap_err();
        assert!(err.contains("FOREIGN KEY") || err.contains("外键") || err.contains("创建待办失败"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 批量更新：改日期/类别/优先级/清颜色、Some(None) 清 date 进收集箱、校验拒绝、不在批次内的不受影响
    #[test]
    fn batch_update_patches_fields_and_clears_date() {
        let (conn, root) = temp_db("batch-upd");
        let a = create_todo(&conn, mk("A")).unwrap();
        let b = create_todo(&conn, mk("B")).unwrap();
        let c = create_todo(&conn, mk("C")).unwrap();

        // 改日期 + 类别 + 优先级 + 显式 null 清 color
        let n = batch_update_todos(
            &conn,
            &[a.id.clone(), b.id.clone()],
            &BatchTodoPatch {
                date: Some(Some(20260916)),
                category: Some("work".into()),
                priority: Some("high".into()),
                color: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(n, 2);
        for id in [&a.id, &b.id] {
            let t = get_todo(&conn, id).unwrap().unwrap();
            assert_eq!(t.date, Some(20260916));
            assert_eq!(t.category, "work");
            assert_eq!(t.priority, "high");
            assert_eq!(t.color, None);
            assert!(t.updated_at >= a.updated_at);
        }
        // 不在批次内的 C 不受影响
        let c2 = get_todo(&conn, &c.id).unwrap().unwrap();
        assert_eq!(c2.date, None);
        assert_eq!(c2.category, "life");
        assert_eq!(c2.priority, "medium");

        // Some(None) 清 date → 进收集箱
        let n2 = batch_update_todos(
            &conn,
            std::slice::from_ref(&a.id),
            &BatchTodoPatch { date: Some(None), ..Default::default() },
        )
        .unwrap();
        assert_eq!(n2, 1);
        assert_eq!(get_todo(&conn, &a.id).unwrap().unwrap().date, None);

        // 校验：空 ids / 非法 priority / 非法 category / 非法 color / 非法 date / 项目不存在
        assert!(batch_update_todos(&conn, &[], &BatchTodoPatch::default()).is_err());
        assert!(batch_update_todos(
            &conn,
            std::slice::from_ref(&a.id),
            &BatchTodoPatch { priority: Some("urgent".into()), ..Default::default() }
        )
        .is_err());
        assert!(batch_update_todos(
            &conn,
            std::slice::from_ref(&a.id),
            &BatchTodoPatch { category: Some("school".into()), ..Default::default() }
        )
        .is_err());
        assert!(batch_update_todos(
            &conn,
            std::slice::from_ref(&a.id),
            &BatchTodoPatch { color: Some(Some("pink".into())), ..Default::default() }
        )
        .is_err());
        assert!(batch_update_todos(
            &conn,
            std::slice::from_ref(&a.id),
            &BatchTodoPatch { date: Some(Some(20261340)), ..Default::default() }
        )
        .is_err());
        assert!(batch_update_todos(
            &conn,
            std::slice::from_ref(&a.id),
            &BatchTodoPatch { project_id: Some(Some("ghost".into())), ..Default::default() }
        )
        .is_err());

        // 项目存在时批量挂项目成功
        conn.execute(
            "INSERT INTO projects (id,name,description,color,sort_order,archived,created_at,updated_at)
             VALUES ('p1','P1',NULL,NULL,0,0,1,1)",
            [],
        )
        .unwrap();
        let n3 = batch_update_todos(
            &conn,
            std::slice::from_ref(&a.id),
            &BatchTodoPatch { project_id: Some(Some("p1".into())), ..Default::default() },
        )
        .unwrap();
        assert_eq!(n3, 1);
        assert_eq!(get_todo(&conn, &a.id).unwrap().unwrap().project_id.as_deref(), Some("p1"));
        let _ = std::fs::remove_dir_all(root);
    }

    /// 批量更新 completed：分配人标记先行（COALESCE 保留原 completed_at）、整体随之、取消全清
    #[test]
    fn batch_update_completed_semantics_with_assignees() {
        let (conn, root) = temp_db("batch-done");
        conn.execute(
            "INSERT INTO people (id,name,note,sort_order,archived,created_at,updated_at)
             VALUES ('p1','张三',NULL,0,0,1,1)",
            [],
        )
        .unwrap();
        let t = create_todo(&conn, mk("协作任务")).unwrap();
        conn.execute(
            "INSERT INTO todo_assignees (todo_id,person_id,completed,completed_at) VALUES (?1,'p1',0,NULL)",
            params![t.id],
        )
        .unwrap();

        // 批量完成 → 分配标记 + 整体都完成
        let n = batch_update_todos(
            &conn,
            std::slice::from_ref(&t.id),
            &BatchTodoPatch { completed: Some(true), ..Default::default() },
        )
        .unwrap();
        assert_eq!(n, 1);
        let (tc, ta, ta_at): (i64, i64, Option<i64>) = conn
            .query_row(
                "SELECT t.completed, ta.completed, ta.completed_at
                 FROM todos t JOIN todo_assignees ta ON ta.todo_id=t.id WHERE t.id=?1",
                params![t.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((tc, ta), (1, 1));
        assert!(ta_at.is_some());

        // 已有 completed_at 时再批量完成：COALESCE 保留原值
        let old_at = get_todo(&conn, &t.id).unwrap().unwrap().completed_at.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5)); // 确保 now 变化
        batch_update_todos(
            &conn,
            std::slice::from_ref(&t.id),
            &BatchTodoPatch { completed: Some(true), ..Default::default() },
        )
        .unwrap();
        let at = get_todo(&conn, &t.id).unwrap().unwrap().completed_at.unwrap();
        assert_eq!(at, old_at, "COALESCE 应保留原 completed_at");

        // 批量取消 → 分配标记与整体全清
        batch_update_todos(
            &conn,
            std::slice::from_ref(&t.id),
            &BatchTodoPatch { completed: Some(false), ..Default::default() },
        )
        .unwrap();
        let (tc, ta, ta_at): (i64, i64, Option<i64>) = conn
            .query_row(
                "SELECT t.completed, ta.completed, ta.completed_at
                 FROM todos t JOIN todo_assignees ta ON ta.todo_id=t.id WHERE t.id=?1",
                params![t.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((tc, ta), (0, 0));
        assert_eq!(ta_at, None);
        assert!(get_todo(&conn, &t.id).unwrap().unwrap().completed_at.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    /// 批量删除：assignees 靠 CASCADE、tags/goal_links 清悬挂、links 保留作失效态、FTS 清干净
    #[test]
    fn batch_delete_cleans_refs_and_keeps_links() {
        let (conn, root) = temp_db("batch-del");
        conn.execute(
            "INSERT INTO people (id,name,note,sort_order,archived,created_at,updated_at)
             VALUES ('p1','张三',NULL,0,0,1,1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO goals (id,title,category,description,status,sort_order,created_at,updated_at)
             VALUES ('g1','目标',NULL,NULL,'active',0,1,1)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO tags (id,name) VALUES ('t1','会议')", []).unwrap();

        let a = create_todo(&conn, mk("删除A")).unwrap();
        let b = create_todo(&conn, mk("删除B")).unwrap();
        let keep = create_todo(&conn, mk("保留")).unwrap();
        for id in [&a.id, &b.id] {
            conn.execute(
                "INSERT INTO todo_assignees (todo_id,person_id,completed,completed_at) VALUES (?1,'p1',0,NULL)",
                params![id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO item_tags (tag_id,item_type,item_id) VALUES ('t1','todo',?1)",
                params![id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO goal_links (goal_id,item_type,item_id,created_at) VALUES ('g1','todo',?1,1)",
                params![id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO links (id,from_type,from_id,to_type,to_id,relation,created_at)
                 VALUES (?1,'todo',?2,'note','n1','reference',1)",
                params![id, id],
            )
            .unwrap();
        }

        // 含一个不存在的 id：只计真实删除的
        let n = batch_delete_todos(&conn, &[a.id.clone(), b.id.clone(), "ghost".into()]).unwrap();
        assert_eq!(n, 2);

        for id in [&a.id, &b.id] {
            let cnt = |sql: &str| -> i64 {
                conn.query_row(sql, params![id], |r| r.get(0)).unwrap()
            };
            assert_eq!(cnt("SELECT COUNT(*) FROM todos WHERE id=?1"), 0);
            assert_eq!(cnt("SELECT COUNT(*) FROM todo_assignees WHERE todo_id=?1"), 0, "assignees 应随 CASCADE 消失");
            assert_eq!(cnt("SELECT COUNT(*) FROM item_tags WHERE item_type='todo' AND item_id=?1"), 0);
            assert_eq!(cnt("SELECT COUNT(*) FROM goal_links WHERE item_type='todo' AND item_id=?1"), 0);
            assert_eq!(cnt("SELECT COUNT(*) FROM todos_fts WHERE id=?1"), 0, "FTS 应清干净");
            // links 有意保留作失效态
            assert_eq!(cnt("SELECT COUNT(*) FROM links WHERE from_type='todo' AND from_id=?1"), 1, "links 应保留");
        }
        assert!(get_todo(&conn, &keep.id).unwrap().is_some());
        // 空 ids 报错
        assert!(batch_delete_todos(&conn, &[]).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
