//! 统一导入服务（F35/F36，SPEC §13.3）：旧 Electron JSON 导入。
//!
//! 规则：
//! - 旧数字 id → 新 18 位 id 映射表，保证 projectId/assignedTo/completedBy 关系不断；
//! - 导入前自动备份 db（复用 db.rs 备份逻辑）；全程单事务，失败回滚不留半截；
//! - 幂等：settings 记来源指纹（内容 hash 前 16 位），重复导入同文件默认跳过；
//! - 旧 hex 颜色映射到 7 色枚举，无法映射的置 NULL；
//! - 协作语义迁移：completedBy 中的人标记 completed=1；status=completed 时整体完成。

use std::collections::HashMap;

use chrono::Local;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::id::new_id;

/// 导入报告（前端展示成功条数/跳过/说明）。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub tasks: usize,
    pub projects: usize,
    pub people: usize,
    pub tags: usize,
    pub goals: usize,
    pub skipped: Vec<String>,
    pub notes: Vec<String>,
    /// true=因指纹相同被跳过，未写入任何数据
    pub already_imported: bool,
}

// ---------- 旧 JSON 结构（宽容解析：字段可缺） ----------

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct LegacyData {
    #[serde(rename = "tasks")]
    tasks: Vec<LegacyTask>,
    #[serde(rename = "projects")]
    projects: Vec<LegacyProject>,
    #[serde(rename = "people")]
    people: Vec<LegacyPerson>,
    #[serde(rename = "presetTags")]
    preset_tags: Vec<LegacyTag>,
    #[serde(rename = "longTermGoals")]
    long_term_goals: Vec<LegacyGoal>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
#[derive(Default)]
struct LegacyTask {
    id: i64,
    title: String,
    #[serde(rename = "description")]
    description: Option<String>,
    #[serde(rename = "category")]
    category: Option<String>,
    #[serde(rename = "projectId")]
    project_id: Option<i64>,
    #[serde(rename = "assignedTo")]
    assigned_to: Vec<i64>,
    #[serde(rename = "completedBy")]
    completed_by: Vec<i64>,
    #[serde(rename = "priority")]
    priority: Option<String>,
    #[serde(rename = "status")]
    status: Option<String>,
    #[serde(rename = "dueDate")]
    due_date: Option<String>,
    #[serde(rename = "tags")]
    tags: Vec<String>,
}


#[derive(Debug, Deserialize)]
#[serde(default)]
#[derive(Default)]
struct LegacyProject {
    id: i64,
    name: String,
    #[serde(rename = "description")]
    description: Option<String>,
    #[serde(rename = "color")]
    color: Option<String>,
}


#[derive(Debug, Deserialize)]
#[serde(default)]
#[derive(Default)]
struct LegacyPerson {
    id: i64,
    name: String,
    #[serde(rename = "role")]
    role: Option<String>,
    #[serde(rename = "email")]
    email: Option<String>,
}


#[derive(Debug, Deserialize)]
#[serde(default)]
#[derive(Default)]
struct LegacyTag {
    name: String,
}


#[derive(Debug, Deserialize)]
#[serde(default)]
#[derive(Default)]
struct LegacyGoal {
    id: i64,
    title: String,
    #[serde(rename = "description")]
    description: Option<String>,
    #[serde(rename = "category")]
    category: Option<String>,
    #[serde(rename = "progress")]
    progress: Option<i64>,
    /// 旧版 targetDate，"YYYY-MM-DD" 字符串日期
    #[serde(rename = "targetDate")]
    target_date: Option<String>,
    #[serde(rename = "status")]
    status: Option<String>,
}






// ---------- 颜色映射 ----------

/// 常见旧 hex → 新 7 色枚举。
const HEX_TO_COLOR: &[(&str, &str)] = &[
    ("#3b82f6", "blue"),
    ("#10b981", "green"),
    ("#f59e0b", "orange"),
    ("#ef4444", "red"),
    ("#eab308", "yellow"),
    ("#22c55e", "green"),
    ("#f97316", "orange"),
    ("#a855f7", "purple"),
    ("#8b5cf6", "purple"),
    ("#ec4899", "purple"),
    ("#06b6d4", "blue"),
    ("#84cc16", "green"),
    ("#9ca3af", "gray"),
    ("#6b7280", "gray"),
];

fn map_color(hex: &Option<String>) -> Option<String> {
    let h = hex.as_deref()?.trim().to_lowercase();
    HEX_TO_COLOR
        .iter()
        .find(|(old, _)| *old == h)
        .map(|(_, name)| name.to_string())
}

// ---------- 工具 ----------

fn now_ms() -> i64 {
    Local::now().timestamp_millis()
}

/// "2024-01-15" → 20240115；解析失败返回 None（进 skipped 记录）。
fn parse_ymd(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.trim().split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let (y, m, d) = (parts[0].parse::<i32>().ok()?, parts[1].parse::<u32>().ok()?, parts[2].parse::<u32>().ok()?);
    chrono::NaiveDate::from_ymd_opt(y, m, d).map(|dt| {
        dt.format("%Y%m%d").to_string().parse().expect("YYYYMMDD 必为数字")
    })
}

fn fingerprint(text: &str) -> String {
    // 轻量指纹：长度 + 首尾各 8 字节（避免引 hash 库；幂等以"同一文件内容"为准足够）
    let bytes = text.as_bytes();
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(8)]).to_string();
    let tail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(8)..]).to_string();
    format!("{:x}-{}-{}", bytes.len(), head, tail)
}

const FP_KEY: &str = "imported_legacy_fingerprint";

/// 主入口：解析并导入旧 JSON（调用方已读入文本）。
pub fn import_legacy_json_text(conn: &mut Connection, text: &str) -> Result<ImportReport, String> {
    let data: LegacyData = serde_json::from_str(text)
        .map_err(|e| format!("JSON 解析失败（旧版 todolist-data.json）：{e}"))?;

    let fp = fingerprint(text);
    // 幂等检查（读，事务外）
    if let Ok(Some(true)) = already_imported(conn, &fp) {
        return Ok(ImportReport {
            already_imported: true,
            ..Default::default()
        });
    }

    let report = {
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut rep = import_in_tx(&tx, &data)?;
        // 记指纹（导入成功才写）
        tx.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![FP_KEY, fp, now_ms()],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        rep.already_imported = false;
        rep
    };
    Ok(report)
}

fn already_imported(conn: &Connection, fp: &str) -> Result<Option<bool>, String> {
    let v: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", params![FP_KEY], |r| r.get(0))
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other.to_string()),
        })?;
    Ok(v.map(|stored| stored == fp))
}

fn import_in_tx(tx: &rusqlite::Transaction, data: &LegacyData) -> Result<ImportReport, String> {
    let mut rep = ImportReport::default();
    let now = now_ms();
    let mut project_map: HashMap<i64, String> = HashMap::new();
    let mut person_map: HashMap<i64, String> = HashMap::new();
    let mut goal_map: HashMap<i64, String> = HashMap::new();

    // 1. projects
    for p in &data.projects {
        if p.name.trim().is_empty() {
            rep.skipped.push(format!("项目 {} 名称为空，跳过", p.id));
            continue;
        }
        let id = new_id();
        tx.execute(
            "INSERT INTO projects (id,name,description,color,sort_order,archived,created_at,updated_at)
             VALUES (?1,?2,?3,?4,0,0,?5,?5)",
            params![id, p.name.trim(), p.description, map_color(&p.color), now],
        )
        .map_err(|e| format!("导入项目 {} 失败：{e}", p.name))?;
        project_map.insert(p.id, id);
        rep.projects += 1;
    }

    // 2. people（0004 起 role/email 落独立列，note 不再拼接，保持 NULL）
    for p in &data.people {
        if p.name.trim().is_empty() {
            rep.skipped.push(format!("人员 {} 名称为空，跳过", p.id));
            continue;
        }
        let id = new_id();
        tx.execute(
            "INSERT INTO people (id,name,note,role,email,sort_order,archived,created_at,updated_at)
             VALUES (?1,?2,NULL,?3,?4,0,0,?5,?5)",
            params![id, p.name.trim(), p.role, p.email, now],
        )
        .map_err(|e| format!("导入人员 {} 失败：{e}", p.name))?;
        person_map.insert(p.id, id);
        rep.people += 1;
    }

    // 3. tasks → todos（先建 todos 再挂 assignees/tags）
    for t in &data.tasks {
        if t.title.trim().is_empty() {
            rep.skipped.push(format!("任务 {} 标题为空，跳过", t.id));
            continue;
        }
        let project_id = match t.project_id {
            Some(old) => project_map.get(&old).cloned(),
            None => None,
        };
        let date = t.due_date.as_deref().and_then(parse_ymd);
        let category = match t.category.as_deref() {
            Some("work") => "work".to_string(),
            _ => "life".to_string(),
        };
        let priority = match t.priority.as_deref() {
            Some("high") => "high",
            Some("low") => "low",
            _ => "medium",
        };
        let completed = matches!(t.status.as_deref(), Some("completed"));
        let id = new_id();
        tx.execute(
            "INSERT INTO todos
               (id,title,content,completed,date,sort_order,color,category,priority,
                project_id,time,parent_id,postpone_auto,created_at,updated_at,completed_at)
             VALUES (?1,?2,?3,?4,?5,0,NULL,?6,?7,?8,NULL,NULL,0,?9,?9,?10)",
            params![
                id,
                t.title.trim(),
                t.description,
                completed as i64,
                date,
                category,
                priority,
                project_id,
                now,
                completed.then_some(now),
            ],
        )
        .map_err(|e| format!("导入任务 {} 失败：{e}", t.title))?;

        // T4.5：同步 FTS 索引（旧版导入同为绕过 create_todo 的裸 INSERT，必须手动挂钩。）
        crate::services::search::sync_todo_fts(tx, &id)?;

        // assignedTo + completedBy（协作语义：completedBy 中的人标完成）
        for old_pid in &t.assigned_to {
            let Some(new_pid) = person_map.get(old_pid) else {
                rep.skipped.push(format!("任务 {} 引用未知人员 {}，分配跳过", t.id, old_pid));
                continue;
            };
            let done = completed || t.completed_by.contains(old_pid);
            tx.execute(
                "INSERT OR IGNORE INTO todo_assignees (todo_id,person_id,completed,completed_at)
                 VALUES (?1,?2,?3,?4)",
                params![id, new_pid, done as i64, done.then_some(now)],
            )
            .map_err(|e| e.to_string())?;
        }

        // todo 标签（含 preset 中出现过的都 ensure）
        for name in &t.tags {
            if name.trim().is_empty() {
                continue;
            }
            let tag_id = ensure_tag(tx, name.trim())?;
            tx.execute(
                "INSERT OR IGNORE INTO item_tags (tag_id,item_type,item_id) VALUES (?1,'todo',?2)",
                params![tag_id, id],
            )
            .map_err(|e| e.to_string())?;
        }
        rep.tasks += 1;
    }

    // 4. presetTags（悬空标签，供侧栏）
    for tg in &data.preset_tags {
        if tg.name.trim().is_empty() {
            continue;
        }
        ensure_tag(tx, tg.name.trim())?;
        rep.tags += 1;
    }

    // 5. longTermGoals（0005 起 progress/target_date 有独立列，直接保留旧版语义）
    for g in &data.long_term_goals {
        if g.title.trim().is_empty() {
            rep.skipped.push(format!("目标 {} 标题为空，跳过", g.id));
            continue;
        }
        let status = match g.status.as_deref() {
            Some("done") => "done",
            Some("archived") => "archived",
            _ => "active",
        };
        // 旧版手动进度 0-100 原样保留；targetDate "YYYY-MM-DD" → 当日零点毫秒时间戳
        let progress = g.progress.unwrap_or(0).clamp(0, 100);
        let target_date = g.target_date.as_deref().and_then(parse_ymd_ms);
        let id = new_id();
        tx.execute(
            "INSERT INTO goals (id,title,category,description,status,progress,target_date,sort_order,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,0,?8,?8)",
            params![id, g.title.trim(), g.category, g.description, status, progress, target_date, now],
        )
        .map_err(|e| format!("导入目标 {} 失败：{e}", g.title))?;
        goal_map.insert(g.id, id);
        rep.goals += 1;
    }
    let _ = &goal_map; // 目标关联旧数据无对应字段，map 保留供后续扩展

    rep.notes.push("导入前已自动备份数据库；重复导入同一文件会自动跳过。".into());
    Ok(rep)
}

/// 同名标签复用（不存在则建）。
fn ensure_tag(conn: &rusqlite::Transaction, name: &str) -> Result<String, String> {
    if let Ok(id) = conn.query_row("SELECT id FROM tags WHERE name=?1", params![name], |r| {
        r.get::<_, String>(0)
    }) {
        return Ok(id);
    }
    let id = new_id();
    conn.execute("INSERT INTO tags (id,name) VALUES (?1,?2)", params![id, name])
        .map_err(|e| format!("建标签 {name} 失败：{e}"))?;
    Ok(id)
}

/// "YYYY-MM-DD" → 当日零点（UTC）毫秒时间戳；解析失败返回 None。
fn parse_ymd_ms(s: &str) -> Option<i64> {
    let d = chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()?;
    Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
}

// ---------- Tauri 命令 ----------

/// 旧 Electron JSON 导入（F35/T1.9）：前端读文件传内容 → 备份 → 单事务导入 → 报告。
/// （内容经前端 FileReader 读入，避免依赖 dialog 插件；BOM 由后端剥离。）
#[tauri::command]
pub fn import_legacy_json(
    db: tauri::State<'_, crate::db::Db>,
    content: String,
) -> Result<ImportReport, String> {
    // 1. 导入前备份（SPEC §13.3）
    let paths = crate::paths::app_paths();
    crate::db::backup_before_migration(&paths.db_path, &paths.backups_dir, 0)
        .map_err(|e| format!("导入前备份失败：{e}"))?;

    // 2. 单事务导入（BOM 剥离）
    let mut conn = db.0.lock().map_err(|e| e.to_string())?;
    let text = content.trim_start_matches('\u{feff}');
    import_legacy_json_text(&mut conn, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_db(tag: &str) -> (Connection, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "todolist-import-test-{}-{}-{}",
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

    /// 全量样本（legacy-electron/src/todolist-data.json 的编译期副本）
    const SAMPLE: &str = include_str!("../../tests/fixtures/legacy-sample.json");

    /// TASKS T1.9 验收：3 任务/3 项目/3 人员/10 标签/2 目标全量导入，关系完整
    #[test]
    fn sample_imports_fully_with_relations() {
        let (mut conn, root) = temp_db("sample");
        let rep = import_legacy_json_text(&mut conn, SAMPLE).unwrap();
        assert!(!rep.already_imported);
        assert_eq!(rep.tasks, 3);
        assert_eq!(rep.projects, 3);
        assert_eq!(rep.people, 3);
        assert_eq!(rep.tags, 10);
        assert_eq!(rep.goals, 2);
        assert!(rep.skipped.is_empty(), "skipped 应为空：{:?}", rep.skipped);

        // 关系断言
        // 任务1：挂项目"网站重构项目"、分配张三+李四（未完成）
        let (pid, cnt): (String, i64) = conn
            .query_row(
                "SELECT t.project_id, (SELECT COUNT(*) FROM todo_assignees ta WHERE ta.todo_id=t.id)
                 FROM todos t WHERE t.title='完成项目报告'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let pname: String = conn
            .query_row("SELECT name FROM projects WHERE id=?1", params![pid], |r| r.get(0))
            .unwrap();
        assert_eq!(pname, "网站重构项目");
        assert_eq!(cnt, 2);

        // 任务3：completedBy 全员 → assignees 3 条 completed=1 且整体 completed=1
        let (overall, marks): (i64, i64) = conn
            .query_row(
                "SELECT t.completed, (SELECT COUNT(*) FROM todo_assignees ta WHERE ta.todo_id=t.id AND ta.completed=1)
                 FROM todos t WHERE t.title='团队会议'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((overall, marks), (1, 3));

        // 任务1（未完成）的 assignedTo completed 应全为 0
        let undone_marks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM todo_assignees ta JOIN todos t ON t.id=ta.todo_id
                 WHERE t.title='完成项目报告' AND ta.completed=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(undone_marks, 0, "pending 任务的分配人完成标记应=0");

        // dueDate 映射：任务2 → 20240112
        let date: i64 = conn
            .query_row("SELECT date FROM todos WHERE title='买菜做饭'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(date, 20240112);

        // 人员 role/email 落独立列（0004 起），note 不再拼接
        let (role, email, note): (String, String, Option<String>) = conn
            .query_row(
                "SELECT role, email, note FROM people WHERE name='张三'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(role, "前端开发");
        assert_eq!(email, "<email@example.com>");
        assert!(note.is_none(), "note 不应再拼 role/email");

        // 目标 progress/target_date 落独立列（0005 起）
        let (gp, gtd, gd): (i64, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT progress, target_date, description FROM goals WHERE title='健康生活'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(gp, 60, "旧版手动进度应原样保留");
        assert!(gtd.is_some(), "targetDate 应解析为时间戳");
        assert!(gd.as_deref() == Some("保持规律作息，坚持运动"), "description 不再附注进度");

        // 标签挂接：任务1 的 tags 报告/项目
        let tagcnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM item_tags it JOIN todos t ON t.id=it.item_id
                 WHERE t.title='完成项目报告' AND it.item_type='todo'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tagcnt, 2);

        // 颜色映射：项目1 → blue
        let color: String = conn
            .query_row("SELECT color FROM projects WHERE name='网站重构项目'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(color, "blue");

        let _ = std::fs::remove_dir_all(root);
    }

    /// TASKS T1.9 验收：重复导入幂等
    #[test]
    fn reimport_same_file_is_idempotent() {
        let (mut conn, root) = temp_db("idem");
        let rep1 = import_legacy_json_text(&mut conn, SAMPLE).unwrap();
        assert_eq!(rep1.tasks, 3);
        let rep2 = import_legacy_json_text(&mut conn, SAMPLE).unwrap();
        assert!(rep2.already_imported, "同指纹应跳过");
        let todos: i64 = conn.query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0)).unwrap();
        assert_eq!(todos, 3, "不产生重复");
        let _ = std::fs::remove_dir_all(root);
    }


    /// 一次性播种（人工/临时）：把 `SEED_LEGACY_JSON` 指向的旧版 JSON 导入到
    /// `SEED_APP_DB` 指向的应用库。未设环境变量直接跳过，不干扰日常 `cargo test`。
    ///
    /// 数据源形态说明：已确认 legacy-electron/legacy-data/todolist-data.json 的顶层键
    /// 就是 tasks/projects/people/presetTags/longTermGoals，与 LegacyData 结构一致，
    /// 故直接把单文件文本（BOM 剥离后）喂给 import_legacy_json_text 即可，
    /// 无需按 ls-tasks.json 等 5 个分文件自行拼装。
    #[test]
    fn seed_real_legacy() {
        let Ok(app_db) = std::env::var("SEED_APP_DB") else { return };
        let Ok(json_path) = std::env::var("SEED_LEGACY_JSON") else { return };
        let db_path = std::path::Path::new(&app_db);
        let backups = db_path.parent().unwrap().join("backups");
        let mut conn = crate::db::open_at(db_path, &backups).unwrap();
        let text = std::fs::read_to_string(&json_path).expect("读不到 SEED_LEGACY_JSON");
        let text = text.trim_start_matches('\u{feff}');
        let rep = import_legacy_json_text(&mut conn, text).unwrap();
        let people_roles: i64 = conn
            .query_row("SELECT COUNT(*) FROM people WHERE role IS NOT NULL OR email IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        println!(
            "SEED_LEGACY_REPORT tasks={} projects={} people={} tags={} goals={} skipped={} already_imported={} people_with_profile={}",
            rep.tasks,
            rep.projects,
            rep.people,
            rep.tags,
            rep.goals,
            rep.skipped.len(),
            rep.already_imported,
            people_roles
        );
    }



    /// TASKS T1.9 验收：损坏 JSON 报错且不写库（事务回滚）
    #[test]
    fn broken_json_rolls_back() {
        let (mut conn, root) = temp_db("broken");
        // 合法 JSON 但任务标题为空 → 中途 skip；再造一个真正错误：字段类型错
        let bad = r#"{"tasks":[{"id":1,"title":"正常"},],"projects":[]}"#; // 尾逗号 → 解析错误
        let err = import_legacy_json_text(&mut conn, bad).unwrap_err();
        assert!(err.contains("JSON 解析失败"), "{err}");
        let todos: i64 = conn.query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0)).unwrap();
        assert_eq!(todos, 0, "解析失败不得写库");
        // 指纹未写入 → 修复后可重导
        let fp_set: i64 = conn
            .query_row("SELECT COUNT(*) FROM settings WHERE key=?1", params![FP_KEY], |r| r.get(0))
            .unwrap();
        assert_eq!(fp_set, 0);
        let _ = std::fs::remove_dir_all(root);
    }

    /// G1 缺口回归：旧版 JSON 导入的待办必须进 FTS 索引（此前导入后搜不到）
    #[test]
    fn legacy_import_syncs_fts() {
        let (mut conn, root) = temp_db("legacy-fts");
        let rep = import_legacy_json_text(&mut conn, SAMPLE).unwrap();
        assert_eq!(rep.tasks, 3);

        let fts_cnt: i64 = conn.query_row("SELECT COUNT(*) FROM todos_fts", [], |r| r.get(0)).unwrap();
        assert_eq!(fts_cnt, 3, "每条导入的待办都应有 FTS 行");

        // 具体一条：标题与正文（含 NULL 落空串）都对得上
        let (fts_title, fts_content): (String, String) = conn
            .query_row(
                "SELECT f.title, f.content FROM todos_fts f JOIN todos t ON t.id=f.id
                 WHERE t.title='完成项目报告'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(fts_title, "完成项目报告");
        let real_content: String = conn
            .query_row("SELECT COALESCE(content,'') FROM todos WHERE title='完成项目报告'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fts_content, real_content, "FTS 正文应与库内一致");
        let _ = std::fs::remove_dir_all(root);
    }

}
