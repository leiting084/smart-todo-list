//! 导出 Markdown / CSV / HTML（F20/T4.1，SPEC §13.4）。
//!
//! 规则：
//! - 选时间范围（date_from..date_to，YYYYMMDD）+ 范围（project/category）+ 含已完成开关；
//! - **CSV 必带 UTF-8 BOM（EF BB BF）** 防 Excel 乱码；字段标准引号转义（含逗号/换行/引号）；
//! - Markdown 按日期 `## YYYY-MM-DD` 分节，`- [x]`/`- [ ]`，标题后挂 #标签/项目/优先级 emoji；
//! - HTML 单文件内联样式、可直接打印；
//! - 命令返回 (path, 条目数)；文件由前端 dialog save 选定路径后写。

use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde::Deserialize;

use crate::models::Todo;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportFilter {
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub project_id: Option<String>,
    pub category: Option<String>,
    pub include_done: Option<bool>,
}

/// 按 filter 查待办（含标签与项目名；排序=date, sort_order）。
fn query_todos(conn: &Connection, f: &ExportFilter) -> Result<Vec<TodoWithMeta>, String> {
    let mut sql = String::from(
        "SELECT t.id, t.title, t.content, t.completed, t.date, t.sort_order, t.color,
                t.category, t.priority, t.project_id, t.time, t.parent_id, t.postpone_auto,
                t.created_at, t.updated_at, t.completed_at, p.name AS project_name
         FROM todos t LEFT JOIN projects p ON p.id = t.project_id WHERE 1=1",
    );
    let mut vals: Vec<rusqlite::types::Value> = Vec::new();
    macro_rules! bind_range {
        ($col:expr, $op:expr, $v:expr) => {{
            sql.push_str(&format!(" AND t.{col} {op} ?{n}", col = $col, op = $op, n = vals.len()));
            vals.push($v);
        }};
    }
    macro_rules! bind_eq {
        ($col:expr, $v:expr) => {{
            sql.push_str(&format!(" AND t.{col} = ?{n}", col = $col, n = vals.len()));
            vals.push($v);
        }};
    }
    if let Some(a) = f.date_from {
        bind_range!("date", ">=", rusqlite::types::Value::Integer(a));
    }
    if let Some(b) = f.date_to {
        bind_range!("date", "<=", rusqlite::types::Value::Integer(b));
    }
    if let Some(p) = &f.project_id {
        bind_eq!("project_id", rusqlite::types::Value::Text(p.clone()));
    }
    if let Some(c) = &f.category {
        bind_eq!("category", rusqlite::types::Value::Text(c.clone()));
    }
    if f.include_done == Some(false) {
        sql.push_str(" AND t.completed = 0");
    }
    sql.push_str(" ORDER BY t.date, t.sort_order, t.created_at");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(vals), |r| {
            Ok(TodoWithMeta {
                todo: Todo {
                    id: r.get("id")?,
                    title: r.get("title")?,
                    content: r.get("content")?,
                    completed: r.get::<_, i64>("completed")? != 0,
                    date: r.get("date")?,
                    sort_order: r.get("sort_order")?,
                    color: r.get("color")?,
                    category: r.get("category")?,
                    priority: r.get("priority")?,
                    project_id: r.get("project_id")?,
                    time: r.get("time")?,
                    parent_id: r.get("parent_id")?,
                    postpone_auto: r.get::<_, i64>("postpone_auto")? != 0,
                    created_at: r.get("created_at")?,
                    updated_at: r.get("updated_at")?,
                    completed_at: r.get("completed_at")?,
                    tags: Vec::new(),
                },
                project_name: r.get("project_name")?,
                assignee_names: Vec::new(),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    // 标签（一次 IN）
    if !out.is_empty() {
        let ids: Vec<String> = out.iter().map(|t| t.todo.id.clone()).collect();
        let ph = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT it.item_id, t2.name FROM item_tags it JOIN tags t2 ON t2.id=it.tag_id
             WHERE it.item_type='todo' AND it.item_id IN ({ph}) ORDER BY t2.name"
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(ids), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        let mut m: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for r in rows {
            let (id, n) = r.map_err(|e| e.to_string())?;
            m.entry(id).or_default().push(n);
        }
        for t in out.iter_mut() {
            if let Some(v) = m.remove(&t.todo.id) {
                t.todo.tags = v;
            }
        }
    }
    // 分配人员（一次 IN）：协作待办的 assignees 展开为人员名列表，供 CSV 导出
    if !out.is_empty() {
        let ids: Vec<String> = out.iter().map(|t| t.todo.id.clone()).collect();
        let ph = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT ta.todo_id, pe.name FROM todo_assignees ta JOIN people pe ON pe.id=ta.person_id
             WHERE ta.todo_id IN ({ph}) ORDER BY pe.name"
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(ids), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        let mut m: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for r in rows {
            let (id, n) = r.map_err(|e| e.to_string())?;
            m.entry(id).or_default().push(n);
        }
        for t in out.iter_mut() {
            if let Some(v) = m.remove(&t.todo.id) {
                t.assignee_names = v;
            }
        }
    }
    Ok(out)
}

struct TodoWithMeta {
    todo: Todo,
    project_name: Option<String>,
    /// 协作待办的分配人员名（多人时 CSV 里以"、"连接）
    assignee_names: Vec<String>,
}

fn ymd_to_iso(ymd: i64) -> String {
    let s = ymd.to_string();
    format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..8])
}

// ---------- Markdown ----------

pub fn export_markdown(conn: &Connection, path: &str, f: &ExportFilter) -> Result<(String, usize), String> {
    let rows = query_todos(conn, f)?;
    let mut buf = String::from("# 待办导出\n\n");
    // 按日期分节
    // 哨兵：用 Option 区分"尚未遇到任何日期"，让收集箱(i64::MIN)也能输出分节头
    let mut cur_date: Option<i64> = None;
    let mut count = 0usize;
    for t in &rows {
        let date = t.todo.date.unwrap_or(i64::MIN);
        if Some(date) != cur_date {
            if cur_date.is_some() {
                buf.push('\n');
            }
            cur_date = Some(date);
            let head = if date == i64::MIN { "收集箱".to_string() } else { ymd_to_iso(date) };
            buf.push_str(&format!("## {head}\n"));
        }
        let mark = if t.todo.completed { "[x]" } else { "[ ]" };
        let prio = match t.todo.priority.as_str() {
            "high" => "🔥 ",
            "low" => "🌱 ",
            _ => "",
        };
        let project = t.project_name.as_deref().map(|p| format!("`{p}`")).unwrap_or_default();
        let tags = t
            .todo
            .tags
            .iter()
            .map(|x| format!("#{x}"))
            .collect::<Vec<_>>()
            .join(" ");
        let extra = [project, tags].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        buf.push_str(&format!(
            "- {mark} {prio}{title}{extra}\n",
            title = t.todo.title,
            extra = if extra.is_empty() { String::new() } else { format!(" — {extra}") },
        ));
        count += 1;
    }
    fs::create_dir_all(Path::new(path).parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    fs::write(path, buf).map_err(|e| format!("写文件失败：{e}"))?;
    Ok((path.to_string(), count))
}

// ---------- CSV（UTF-8 BOM + 标准转义） ----------

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn export_csv(conn: &Connection, path: &str, f: &ExportFilter) -> Result<(String, usize), String> {
    let rows = query_todos(conn, f)?;
    let mut buf = Vec::new();
    // BOM：防 Excel 中文乱码（SPEC 强要求）
    buf.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    buf.extend_from_slice(
        b"id,title,content,category,priority,project,assignees,tags,date,time,completed,completed_at\n",
    );
    let mut count = 0usize;
    for t in &rows {
        let line = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv_escape(&t.todo.id),
            csv_escape(&t.todo.title),
            csv_escape(t.todo.content.as_deref().unwrap_or("")),
            csv_escape(&t.todo.category),
            csv_escape(&t.todo.priority),
            csv_escape(t.project_name.as_deref().unwrap_or("")),
            csv_escape(&t.assignee_names.join("、")),
            csv_escape(&t.todo.tags.join(" ")),
            t.todo.date.map(|d| d.to_string()).unwrap_or_default(),
            t.todo.time.as_deref().unwrap_or(""),
            if t.todo.completed { "1" } else { "0" },
            t.todo.completed_at.map(|m| m.to_string()).unwrap_or_default(),
        );
        buf.extend_from_slice(line.as_bytes());
        count += 1;
    }
    fs::create_dir_all(Path::new(path).parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    fs::write(path, buf).map_err(|e| format!("写文件失败：{e}"))?;
    Ok((path.to_string(), count))
}

// ---------- HTML（单文件内联样式可打印） ----------

pub fn export_html(conn: &Connection, path: &str, f: &ExportFilter) -> Result<(String, usize), String> {
    let rows = query_todos(conn, f)?;
    let mut body = String::new();
    let mut count = 0usize;
    for t in &rows {
        let prio_badge = match t.todo.priority.as_str() {
            "high" => r#"<span class="p p-high">高</span>"#,
            "low" => r#"<span class="p p-low">低</span>"#,
            _ => r#"<span class="p">中</span>"#,
        };
        let tags = t.todo.tags.iter().map(|x| format!(r#"<span class="tag">#{x}</span>"#)).collect::<Vec<_>>().join(" ");
        body.push_str(&format!(
            r#"
<div class="item {done}">
  <label><input type="checkbox" {checked}/> <span class="t">{title}</span></label>
  <div class="meta">{prio}{project}{info}{tags}</div>
  <div class="c">{content}</div>
</div>"#,
            done = if t.todo.completed { "done" } else { "" },
            checked = if t.todo.completed { "checked" } else { "" },
            title = html_escape(&t.todo.title),
            prio = prio_badge,
            project = t
                .project_name
                .as_deref()
                .map(|p| format!(r#"<span class="prj" title="项目">{}</span>"#, html_escape(p)))
                .unwrap_or_default(),
            info = t.todo.date.map(|d| format!(r#"<span class="date">📅 {}</span>"#, ymd_to_iso(d))).unwrap_or_default(),
            tags = tags,
            content = html_escape(t.todo.content.as_deref().unwrap_or("")),
        ));
        count += 1;
    }
    let html = format!(
        r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>待办导出</title>
<style>
 body {{ font-family: "Microsoft YaHei", sans-serif; margin: 24px; color: #222; }}
 h1 {{ font-size: 20px; }}
 .item {{ border-bottom: 1px solid #eee; padding: 8px 0; }}
 .item.done .t {{ text-decoration: line-through; color: #999; }}
 .p {{ font-size: 12px; border: 1px solid #ccc; border-radius: 4px; padding: 0 4px; margin-right: 6px; }}
 .p-high {{ color: #c0392b; border-color: #c0392b; }}
 .tag {{ color: #0d9488; margin-right: 6px; font-size: 12px; }}
 .prj, .date {{ color: #888; margin-right: 6px; font-size: 12px; }}
 .c {{ font-size: 13px; color: #555; margin-top: 4px; white-space: pre-wrap; }}
 @media print {{ body {{ margin: 0; }} }}
</style></head><body><h1>待办导出</h1>{body}</body></html>"#
    );
    fs::create_dir_all(Path::new(path).parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    fs::write(path, html).map_err(|e| format!("写文件失败：{e}"))?;
    Ok((path.to_string(), count))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ---------- 命令 ----------

#[tauri::command]
pub fn export_todos_markdown(
    db: tauri::State<'_, crate::db::Db>,
    path: String,
    filter: Option<ExportFilter>,
) -> Result<(String, usize), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    export_markdown(&conn, &path, &filter.unwrap_or_default())
}

#[tauri::command]
pub fn export_todos_csv(
    db: tauri::State<'_, crate::db::Db>,
    path: String,
    filter: Option<ExportFilter>,
) -> Result<(String, usize), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    export_csv(&conn, &path, &filter.unwrap_or_default())
}

#[tauri::command]
pub fn export_todos_html(
    db: tauri::State<'_, crate::db::Db>,
    path: String,
    filter: Option<ExportFilter>,
) -> Result<(String, usize), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    export_html(&conn, &path, &filter.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::todo::create_todo;
    use crate::commands::tag::set_item_tags;
    use crate::models::ItemType;
    use crate::commands::project::create_project;
    use crate::models::CreateProject;
    use crate::models::CreateTodo;
    use std::path::PathBuf;

    fn temp(tag: &str) -> (Connection, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-export-test-{}-{}-{}",
            std::process::id(),
            std::line!(),
            tag
        ));
        let _ = fs::remove_dir_all(&root);
        let bkp = root.join("backups");
        fs::create_dir_all(&bkp).unwrap();
        let conn = crate::db::open_at(&root.join("todolist.db"), &bkp).unwrap();
        (conn, root)
    }

    fn mk(conn: &Connection, title: &str, date: Option<i64>) -> std::string::String {
        create_todo(conn, CreateTodo {
            title: title.into(),
            content: None,
            date,
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

    /// TASKS 验收：CSV BOM 头 + 中文/逗号/换行/引号转义；Excel 打开不乱码
    #[test]
    fn csv_bom_and_escaping() {
        let (conn, root) = temp("csv");
        let conn = &conn;
        mk(conn, "普通,带逗号\"和引号", Some(20260918));
        let path = root.join("out.csv").to_str().unwrap().to_string();
        let (_, n) = export_csv(conn, &path, &ExportFilter::default()).unwrap();
        assert_eq!(n, 1);
        let raw = fs::read(root.join("out.csv")).unwrap();
        // BOM：EF BB BF
        assert_eq!(&raw[0..3], &[0xEF, 0xBB, 0xBF], "必须带 UTF-8 BOM");
        let text = String::from_utf8(raw[3..].to_vec()).unwrap();
        assert!(text.contains("\"普通,带逗号\"\"和引号\""), "引号转义：{text}");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 协作待办的 assignees 列：多人展开为人员名（此前该列恒为空白占位）
    #[test]
    fn csv_assignees_column_filled() {
        let (conn, root) = temp("csv-assignees");
        let conn = &conn;
        let id = mk(conn, "协作任务", Some(20260918));
        let p1 = crate::commands::person::create_person(
            conn,
            crate::models::CreatePerson { name: "张三".into(), ..Default::default() },
        )
        .unwrap();
        let p2 = crate::commands::person::create_person(
            conn,
            crate::models::CreatePerson { name: "李四".into(), ..Default::default() },
        )
        .unwrap();
        crate::commands::person::set_assignees(conn, &id, &[p1.id, p2.id]).unwrap();

        let path = root.join("out.csv").to_str().unwrap().to_string();
        export_csv(conn, &path, &ExportFilter::default()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let line = text.lines().nth(1).expect("数据行");
        let cols: Vec<&str> = line.split(',').collect();
        assert!(cols[6].contains("张三") && cols[6].contains("李四"), "两人都应出现在列内：{line}");
        assert!(cols[6].contains("、"), "多人以、连接：{line}");
        assert!(!cols[6].is_empty(), "assignees 列不应再为空：{line}");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Markdown：按日期分节 + 完成符号 + 标签/项目
    #[test]
    fn markdown_sections_and_meta() {
        let (conn, root) = temp("md");
        let conn = &conn;
        let p = create_project(conn, CreateProject { name: "官网".into(), ..Default::default() }).unwrap();
        let id = {
            let c = CreateTodo {
                title: "改版落地".into(),
                content: None,
                date: Some(20260918),
                color: None,
                category: None,
                priority: Some("high".into()),
                project_id: Some(p.id.clone()),
                time: None,
                parent_id: None,
                sort_order: None,
            };
            create_todo(conn, c.clone()).unwrap().id
        };
        set_item_tags(conn, ItemType::Todo, &id, &["上线".into()]).unwrap();
        create_todo(conn, CreateTodo {
            title: "收集任务".into(),
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
        let path = root.join("out.md").to_str().unwrap().to_string();
        let (_, n) = export_markdown(conn, &path, &ExportFilter::default()).unwrap();
        assert_eq!(n, 2);
        let text = fs::read_to_string(root.join("out.md")).unwrap();
        assert!(text.contains("## 2026-09-18"), "日期分节");
        assert!(text.contains("## 收集箱"));
        assert!(text.contains("- [ ] 🔥 改版落地 — `官网` #上线"), "优先级+项目+标签：{text}");
        // 完成项标记
        crate::commands::todo::set_completed(conn, &id, true).unwrap();
        let (_, _) = export_markdown(conn, &path, &ExportFilter::default()).unwrap();
        let text = fs::read_to_string(root.join("out.md")).unwrap();
        assert!(text.contains("- [x] 🔥 改版落地"), "[x] 完成标记");
        let _ = std::fs::remove_dir_all(root);
    }

    /// HTML：转义 + 结构
    #[test]
    fn html_escapes_and_structure() {
        let (conn, root) = temp("html");
        let conn = &conn;
        mk(conn, "<脚本>&&", Some(20260918));
        let path = root.join("out.html").to_str().unwrap().to_string();
        let (_, n) = export_html(conn, &path, &ExportFilter::default()).unwrap();
        assert_eq!(n, 1);
        let text = fs::read_to_string(root.join("out.html")).unwrap();
        assert!(text.contains("&lt;脚本&gt;&amp;&amp;"), "HTML 转义");
        assert!(text.contains("<!doctype html>"));
        let _ = std::fs::remove_dir_all(root);
    }
}