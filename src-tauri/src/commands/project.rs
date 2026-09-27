//! 项目管理命令（F30，继承旧版）：create/update/archive/list + 统计。
//!
//! 删除语义=归档（archived=1），不物理删除；todos.project_id ON DELETE SET NULL 兜底
//! （即便将来物理删，待办也会脱钩而非消失）。

use rusqlite::{params, types::Value, Connection, Row};
use tauri::State;

use crate::db::Db;
use crate::id::new_id;
use crate::models::{
    CreateProject, Project, ProjectStats, UpdateProject, COLORS,
};

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

fn map_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        color: row.get("color")?,
        sort_order: row.get("sort_order")?,
        archived: row.get::<_, i64>("archived")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

const COLUMNS: &str =
    "id, name, description, color, sort_order, archived, created_at, updated_at";

fn validate(name: &str, color: &Option<String>) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("项目名称不能为空".into());
    }
    if let Some(c) = color {
        if !COLORS.contains(&c.as_str()) {
            return Err(format!("color 必须是 {COLORS:?}，实际 {c}"));
        }
    }
    Ok(())
}

// ---------- repo ----------

pub fn create_project(conn: &Connection, input: CreateProject) -> Result<Project, String> {
    validate(&input.name, &input.color)?;
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO projects (id,name,description,color,sort_order,archived,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,0,?6,?6)",
        params![id, input.name.trim(), input.description, input.color, input.sort_order.unwrap_or(0), now],
    )
    .map_err(|e| format!("创建项目失败：{e}"))?;
    get_project(conn, &id)
}

pub fn get_project(conn: &Connection, id: &str) -> Result<Project, String> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM projects WHERE id=?1"), params![id], map_row)
        .map_err(|e| e.to_string())
}

pub fn update_project(conn: &Connection, id: &str, input: UpdateProject) -> Result<Project, String> {
    if get_project(conn, id).is_err() {
        return Err(format!("项目不存在：{id}"));
    }
    if let Some(n) = &input.name {
        if n.trim().is_empty() {
            return Err("项目名称不能为空".into());
        }
    }
    if let Some(Some(c)) = &input.color {
        // 前端用空串 "" 表示「清除颜色」（JSON null 会被 serde 折叠成"不改"，无法区分）
        if !c.is_empty() && !COLORS.contains(&c.as_str()) {
            return Err(format!("color 必须是 {COLORS:?}，实际 {c}"));
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
    if let Some(d) = input.description {
        let v = match d.as_deref() {
            Some(s) if !s.is_empty() => Value::Text(s.to_string()),
            _ => Value::Null, // None（不改）不会走到这；Some(None) 或 Some("") → 清空
        };
        push("description", v);
    }
    if let Some(c) = input.color {
        let v = match c.as_deref() {
            Some(s) if !s.is_empty() => Value::Text(s.to_string()),
            _ => Value::Null, // Some("") → 清除颜色
        };
        push("color", v);
    }
    if let Some(s) = input.sort_order {
        push("sort_order", Value::Integer(s));
    }
    if sets.is_empty() {
        return get_project(conn, id);
    }
    sets.push(format!("updated_at = ?{}", vals.len() + 1));
    vals.push(Value::Integer(now_ms()));
    vals.push(Value::Text(id.to_string()));
    conn.execute(
        &format!("UPDATE projects SET {} WHERE id=?{}", sets.join(", "), vals.len()),
        rusqlite::params_from_iter(vals),
    )
    .map_err(|e| e.to_string())?;
    get_project(conn, id)
}

/// 归档/恢复。归档后项目不出现在选择器，但其下待办仍可查（project_id 保留）。
pub fn set_archived(conn: &Connection, id: &str, archived: bool) -> Result<(), String> {
    let n = conn
        .execute(
            "UPDATE projects SET archived=?2, updated_at=?3 WHERE id=?1",
            params![id, archived as i64, now_ms()],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("项目不存在：{id}"));
    }
    Ok(())
}

/// 列表；默认不含已归档。
pub fn list_projects(conn: &Connection, include_archived: bool) -> Result<Vec<Project>, String> {
    let sql = format!(
        "SELECT {COLUMNS} FROM projects {} ORDER BY archived ASC, sort_order ASC, created_at ASC",
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

/// 项目下待办统计（总数/完成数/完成率）。
pub fn project_stats(conn: &Connection, id: &str) -> Result<ProjectStats, String> {
    let (total, done): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(completed),0) FROM todos WHERE project_id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    Ok(ProjectStats {
        todo_count: total,
        completed_count: done,
        completion_rate: if total == 0 { 0.0 } else { done as f64 / total as f64 },
    })
}

// ---------- Tauri 命令 ----------

#[tauri::command]
pub fn project_create(db: State<'_, Db>, input: CreateProject) -> Result<Project, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let project = create_project(&conn, input)?;
    let _ = crate::services::oplog::record(&conn, "create", "project", Some(&project.id), &project.name);
    Ok(project)
}

#[tauri::command]
pub fn project_update(db: State<'_, Db>, id: String, input: UpdateProject) -> Result<Project, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let project = update_project(&conn, &id, input)?;
    let _ = crate::services::oplog::record(&conn, "update", "project", Some(&project.id), &project.name);
    Ok(project)
}

#[tauri::command]
pub fn project_archive(db: State<'_, Db>, id: String, archived: bool) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let name = get_project(&conn, &id).map(|p| p.name).unwrap_or_else(|_| id.clone());
    set_archived(&conn, &id, archived)?;
    let action = if archived { "archive" } else { "unarchive" };
    let _ = crate::services::oplog::record(&conn, action, "project", Some(&id), &name);
    Ok(())
}

/// 删除=归档（SPEC：UI 不物理删）。
#[tauri::command]
pub fn project_delete(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let name = get_project(&conn, &id).map(|p| p.name).unwrap_or_else(|_| id.clone());
    set_archived(&conn, &id, true)?;
    let _ = crate::services::oplog::record(&conn, "delete", "project", Some(&id), &name);
    Ok(())
}

#[tauri::command]
pub fn project_list(db: State<'_, Db>, include_archived: Option<bool>) -> Result<Vec<Project>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list_projects(&conn, include_archived.unwrap_or(false))
}

#[tauri::command]
pub fn project_get_stats(db: State<'_, Db>, id: String) -> Result<ProjectStats, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    project_stats(&conn, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::todo::{create_todo, set_completed};
    use crate::models::CreateTodo;

    fn temp_db(tag: &str) -> (Connection, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-proj-test-{}-{}-{}",
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

    fn mk_todo(conn: &Connection, title: &str, project_id: Option<&str>, completed: bool) -> String {
        let t = create_todo(conn, CreateTodo {
            title: title.into(),
            content: None,
            date: None,
            color: None,
            category: Some("work".into()),
            priority: None,
            project_id: project_id.map(String::from),
            time: None,
            parent_id: None,
            sort_order: None,
        })
        .unwrap();
        if completed {
            set_completed(conn, &t.id, true).unwrap();
        }
        t.id
    }

    #[test]
    fn two_projects_counts_and_rate() {
        let (conn, root) = temp_db("stats");
        let p1 = create_project(&conn, CreateProject {
            name: "官网改版".into(),
            description: None,
            color: Some("blue".into()),
            sort_order: None,
        })
        .unwrap();
        let p2 = create_project(&conn, CreateProject {
            name: "移动应用".into(),
            description: None,
            color: None,
            sort_order: None,
        })
        .unwrap();

        // p1：4 待办完成 1 → 25%
        for (i, done) in [true, false, false, false].iter().enumerate() {
            mk_todo(&conn, &format!("p1-{i}"), Some(&p1.id), *done);
        }
        // p2：2 待办全完成 → 100%
        mk_todo(&conn, "p2-a", Some(&p2.id), true);
        mk_todo(&conn, "p2-b", Some(&p2.id), true);

        let s1 = project_stats(&conn, &p1.id).unwrap();
        assert_eq!((s1.todo_count, s1.completed_count), (4, 1));
        assert!((s1.completion_rate - 0.25).abs() < 1e-9);
        let s2 = project_stats(&conn, &p2.id).unwrap();
        assert_eq!((s2.todo_count, s2.completed_count), (2, 2));
        assert!((s2.completion_rate - 1.0).abs() < 1e-9);

        // 空项目 0
        let p3 = create_project(&conn, CreateProject { name: "空".into(), ..Default::default() }).unwrap();
        assert_eq!(project_stats(&conn, &p3.id).unwrap().completion_rate, 0.0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn archived_hidden_from_selector_but_todos_remain() {
        let (conn, root) = temp_db("archive");
        let p = create_project(&conn, CreateProject { name: "将归档".into(), ..Default::default() }).unwrap();
        let todo_id = mk_todo(&conn, "历史待办", Some(&p.id), false);

        set_archived(&conn, &p.id, true).unwrap();
        let active = list_projects(&conn, false).unwrap();
        assert!(!active.iter().any(|x| x.id == p.id), "归档项目不出现在默认选择器");
        let all = list_projects(&conn, true).unwrap();
        assert!(all.iter().any(|x| x.id == p.id && x.archived));

        // 历史待办仍可查且仍指向该项目
        let t: (i64, Option<String>) = conn
            .query_row("SELECT completed, project_id FROM todos WHERE id=?1", params![todo_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(t.1, Some(p.id.clone()));
        assert_eq!(t.0, 0);

        // 恢复
        set_archived(&conn, &p.id, false).unwrap();
        assert!(list_projects(&conn, false).unwrap().iter().any(|x| x.id == p.id));

        // 空名称/非法颜色拒绝
        assert!(create_project(&conn, CreateProject { name: "  ".into(), ..Default::default() }).is_err());
        assert!(create_project(&conn, CreateProject { name: "x".into(), color: Some("pink".into()), ..Default::default() }).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn project_delete_is_archive_and_sets_null_fallback() {
        let (conn, root) = temp_db("delete");
        let p = create_project(&conn, CreateProject { name: "P".into(), ..Default::default() }).unwrap();
        mk_todo(&conn, "t", Some(&p.id), false);

        // project_delete 语义=归档
        crate::commands::project::set_archived(&conn, &p.id, true).unwrap();
        assert!(list_projects(&conn, false).unwrap().iter().all(|x| x.id != p.id));

        // 物理删除项目（模拟极端情况）→ todos.project_id 被 SET NULL，待办不消失
        conn.execute("DELETE FROM projects WHERE id=?1", params![p.id]).unwrap();
        let pid: Option<String> = conn
            .query_row("SELECT project_id FROM todos WHERE title='t'", [], |r| r.get(0))
            .unwrap();
        assert!(pid.is_none(), "ON DELETE SET NULL 兜底");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1, "待办本身仍在");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn update_clears_color_and_description_via_empty_string() {
        let (conn, root) = temp_db("clear");
        let p = create_project(&conn, CreateProject {
            name: "带色项目".into(),
            description: Some("描述内容".into()),
            color: Some("blue".into()),
            sort_order: None,
        })
        .unwrap();
        assert_eq!(p.color.as_deref(), Some("blue"));

        // 前端契约：用空串 "" 表示清除（JSON null 会被 serde 折叠成"不改"）
        let after = update_project(&conn, &p.id, UpdateProject {
            color: Some(Some("".into())),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(after.color, None, "空串应把颜色清成 NULL");
        assert_eq!(after.description.as_deref(), Some("描述内容"), "未提供的描述不应被改动");

        // 空串清除描述
        let after2 = update_project(&conn, &p.id, UpdateProject {
            description: Some(Some("".into())),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(after2.description, None, "空串应把描述清成 NULL");
        assert_eq!(after2.color, None, "颜色保持已清除状态");

        // 非法颜色仍被拒
        assert!(update_project(&conn, &p.id, UpdateProject {
            color: Some(Some("pink".into())),
            ..Default::default()
        })
        .is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
