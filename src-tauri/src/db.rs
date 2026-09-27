//! 数据层：SQLite 连接、WAL pragma、版本化迁移、升级前备份（SPEC §5 / F10）。
//!
//! 设计：
//! - 单用户本地应用，`Mutex<Connection>` 同步模型最简单（SPEC §6.1）；
//! - 迁移文件编译期 `include_str!` 嵌入，顺序执行，每条一个事务；
//! - 执行未应用迁移前，若 db 文件已存在则 checkpoint WAL 并复制到
//!   `backups/todolist-YYYYMMDD-HHMMSS-pre-vN.db`，仅保留最近 5 份。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Local;
use rusqlite::Connection;

use crate::paths::AppPaths;

/// 应用共享数据库（Tauri managed state）。
// 字段自 T1.1 起由各命令通过 State<Db>.0.lock() 消费。
#[allow(dead_code)]
pub struct Db(pub Mutex<Connection>);

/// 一条版本化迁移。
pub(crate) struct Migration {
    pub version: i64,
    pub description: &'static str,
    pub sql: &'static str,
}

/// 编译期嵌入的迁移序列，按 version 升序。新增改动只允许追加 000N（SPEC：迁移只增不改）。
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "init",
        sql: include_str!("../migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        description: "repeat_notification",
        sql: include_str!("../migrations/0002_repeat_notification.sql"),
    },
    Migration {
        version: 3,
        description: "fts",
        sql: include_str!("../migrations/0003_fts.sql"),
    },
    Migration {
        version: 4,
        description: "people_profile",
        sql: include_str!("../migrations/0004_people_profile.sql"),
    },
    Migration {
        version: 5,
        description: "goal_profile",
        sql: include_str!("../migrations/0005_goal_profile.sql"),
    },
];

/// 生产入口：在全局 paths 指向的位置打开/创建数据库并迁移到最新。
pub fn open(paths: &AppPaths) -> Result<Db, String> {
    let conn = open_at(&paths.db_path, &paths.backups_dir).map_err(|e| e.to_string())?;
    Ok(Db(Mutex::new(conn)))
}

/// 可对任意路径打开（测试/导入器复用）：设 pragma → 跑迁移。
pub fn open_at(db_path: &Path, backups_dir: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    run_migrations(&mut conn, db_path, backups_dir)?;
    Ok(conn)
}

/// 已应用版本集合；schema_migrations 尚不存在时视为空（0001 会创建它）。
fn applied_versions(conn: &Connection) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT version FROM schema_migrations")?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 顺序执行未应用迁移；每条一个事务，失败整体回滚（旧库保持可读）。
pub fn run_migrations(
    conn: &mut Connection,
    db_path: &Path,
    backups_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let applied = applied_versions(conn)?;
    let now_ms = Local::now().timestamp_millis();

    for m in MIGRATIONS {
        if applied.contains(&m.version) {
            continue;
        }
        // 升级前备份：仅当库已应用过迁移（非全新空库）。
        // 注意不能只判 db 文件存在——Connection::open 会先创建空文件。
        if !applied.is_empty() {
            backup_before_migration(db_path, backups_dir, m.version)?;
        }

        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, description, applied_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![m.version, m.description, now_ms],
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// 把当前 db checkpoint 后复制为带后缀的备份（迁移用 pre-vN；导入器复用为 import 前备份）。
pub(crate) fn backup_before_migration(
    db_path: &Path,
    backups_dir: &Path,
    version: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    if !db_path.exists() {
        return Ok(());
    }
    fs::create_dir_all(backups_dir)?;

    // 让 WAL 中已提交内容落回主库，保证只复制 .db 也是一致快照。
    let ck = Connection::open(db_path)?;
    ck.pragma_update(None, "journal_mode", "WAL")?;
    ck.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(ck);

    let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dest = backups_dir.join(format!("todolist-{stamp}-pre-v{version}.db"));
    fs::copy(db_path, &dest)?;

    prune_backups(backups_dir, 5)?;
    Ok(())
}

/// 保留最近 keep 份 todolist-*-pre-v*.db，删除更旧的。
fn prune_backups(backups_dir: &Path, keep: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut files: Vec<PathBuf> = fs::read_dir(backups_dir)?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("todolist-") && n.contains("-pre-v") && n.ends_with(".db"))
        })
        .collect();
    if files.len() <= keep {
        return Ok(());
    }
    // 文件名含定宽 YYYYMMDD-HHMMSS 时间戳，字典序即时间序（新→旧）。
    files.sort_unstable_by(|a, b| b.file_name().cmp(&a.file_name()));
    for old in &files[keep..] {
        fs::remove_file(old)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "todolist-db-test-{}-{}-{}",
            std::process::id(),
            std::line!(),
            tag
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("backups")).unwrap();
        d
    }

    #[test]
    fn migrate_fresh_then_idempotent() {
        let root = temp_root("fresh");
        let dbp = root.join("data").join("todolist.db");
        let bkp = root.join("backups");

        // 首次：建库 + 全部迁移（当前 5 条：0001-0005）
        {
            let conn = open_at(&dbp, &bkp).expect("fresh migrate");
            let n: i64 = conn
                .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 5, "首次迁移后 schema_migrations 应有 5 条（0001-0005）");

            let mode: String = conn
                .pragma_query_value(None, "journal_mode", |r| r.get(0))
                .unwrap();
            assert_eq!(mode.to_lowercase(), "wal");

            let fk: i64 = conn
                .pragma_query_value(None, "foreign_keys", |r| r.get(0))
                .unwrap();
            assert_eq!(fk, 1);

            // 0001 的关键表都在
            for t in [
                "projects", "people", "goals", "todos", "todo_assignees", "goal_links",
                "memos", "notes", "links", "tags", "item_tags", "settings",
            ] {
                let c: i64 = conn
                    .query_row(
                        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                        rusqlite::params![t],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(c, 1, "缺少表 {t}");
            }
            // 0002 已建 repeat/notification 表
            for t in ["todo_repeat_rules", "todo_repeat_items", "notification_log"] {
                let c: i64 = conn
                    .query_row(
                        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                        rusqlite::params![t],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(c, 1, "缺少 0002 表 {t}");
            }
            // 0004 已给 people 补 role/email 列
            for col in ["role", "email"] {
                let c: i64 = conn
                    .query_row(
                        "SELECT count(*) FROM pragma_table_info('people') WHERE name=?1",
                        rusqlite::params![col],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(c, 1, "缺少 0004 people 列 {col}");
            }
            // 0005 已给 goals 补 progress/target_date 列
            for col in ["progress", "target_date"] {
                let c: i64 = conn
                    .query_row(
                        "SELECT count(*) FROM pragma_table_info('goals') WHERE name=?1",
                        rusqlite::params![col],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(c, 1, "缺少 0005 goals 列 {col}");
            }
        }

        // 二次打开：不重复执行、不报错；仍 5 条迁移记录，且无备份（没有未应用迁移，不触发备份）。
        {
            let conn = open_at(&dbp, &bkp).expect("reopen");
            let n: i64 = conn
                .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 5);
        }
        let backups = fs::read_dir(&bkp).unwrap().count();
        assert_eq!(backups, 0, "无未应用迁移时不应产生备份");
        let _ = fs::remove_dir_all(&root);
    }

    /// 直接测 runner：构造一个会失败的 v2 迁移，验证事务回滚 + 原库可读 + 生成 pre-v2 备份。
    #[test]
    fn failed_migration_keeps_db_readable_and_backs_up() {
        use crate::db::Migration;

        let root = temp_root("fail");
        let dbp = root.join("todolist.db");
        let bkp = root.join("backups");

        // 先正常跑到 v1，并写入一条项目作为旧数据。
        {
            let conn = open_at(&dbp, &bkp).expect("v1");
            conn.execute(
                "INSERT INTO projects (id,name,description,color,sort_order,archived,created_at,updated_at)
                 VALUES ('p1','旧项目',NULL,NULL,0,0,1,1)",
                [],
            )
            .unwrap();
            drop(conn);
        }

        // 构造含坏 SQL 的假迁移 v6（引用不存在的表）；真实 0001-0005 已自动应用。
        let bad = Migration {
            version: 6,
            description: "bad",
            sql: "CREATE TABLE should_rollback(id INTEGER); INSERT INTO nope VALUES (1);",
        };
        {
            let mut conn = Connection::open(&dbp).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            let err = run_migrations_with(&mut conn, &dbp, &bkp, &[bad]).unwrap_err();
            assert!(err.to_string().to_lowercase().contains("no such table"), "错误应为坏 SQL：{err}");
        }

        // pre-v6 备份已生成。
        let mut backups: Vec<String> = fs::read_dir(&bkp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(backups.len(), 1, "应生成 1 份备份，实际 {backups:?}");
        assert!(backups[0].contains("-pre-v6.db"), "备份名 {backups:?}");

        // 备份文件可打开且含旧数据。
        let backup_path = bkp.join(backups.remove(0));
        {
            let bc = Connection::open(&backup_path).unwrap();
            let name: String = bc
                .query_row("SELECT name FROM projects WHERE id='p1'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(name, "旧项目");
            // 备份是 v1 状态：没有 v2、没有半建表
            let c: i64 = bc
                .query_row("SELECT count(*) FROM sqlite_master WHERE name='should_rollback'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(c, 0);
        }

        // 原库仍可读、v1 数据在、真实迁移已应用、坏 v6 未登记（事务回滚）。
        {
            let conn = Connection::open(&dbp).unwrap();
            let name: String = conn
                .query_row("SELECT name FROM projects WHERE id='p1'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(name, "旧项目");
            let v: i64 = conn
                .query_row("SELECT count(*) FROM schema_migrations WHERE version=6", [], |r| r.get(0))
                .unwrap();
            assert_eq!(v, 0, "失败迁移不得登记版本");
            let ok2: i64 = conn
                .query_row("SELECT count(*) FROM schema_migrations WHERE version=5", [], |r| r.get(0))
                .unwrap();
            assert_eq!(ok2, 1, "真实 0005 应已正常应用");
            let c: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master WHERE name='should_rollback'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(c, 0, "DDL 也应随事务回滚");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_keeps_latest_five_backups() {
        let root = temp_root("prune");
        let bkp = root.join("backups");
        for d in 1..=7 {
            let name = format!("todolist-2026091{d}-101010-pre-v1.db");
            fs::write(bkp.join(name), b"x").unwrap();
        }
        // 混入一个不匹配命名的文件，不能被当作备份删除
        fs::write(bkp.join("keep-me.txt"), b"y").unwrap();

        prune_backups(&bkp, 5).unwrap();

        let mut left: Vec<String> = fs::read_dir(&bkp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        left.sort();
        assert!(left.contains(&"keep-me.txt".to_string()), "非备份文件不得被删");
        let dbs: Vec<&String> = left.iter().filter(|n| n.ends_with(".db")).collect();
        assert_eq!(dbs.len(), 5, "只保留最近 5 份");
        assert!(dbs[0].contains("20260913"), "最旧保留应是 0913，实际 {}", dbs[0]);
        assert!(dbs[4].contains("20260917"), "最新保留应是 0917，实际 {}", dbs[4]);
        let _ = fs::remove_dir_all(&root);
    }

    /// 用自定义迁移集跑 runner（测试钩子）。
    fn run_migrations_with(
        conn: &mut Connection,
        db_path: &Path,
        backups_dir: &Path,
        extra: &[Migration],
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 真实 0001 已应用，这里只跑传入的额外迁移。
        let applied = applied_versions(conn)?;
        let now_ms = Local::now().timestamp_millis();
        for m in extra {
            if applied.contains(&m.version) {
                continue;
            }
            if !applied.is_empty() {
                backup_before_migration(db_path, backups_dir, m.version)?;
            }
            let tx = conn.transaction()?;
            tx.execute_batch(m.sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, description, applied_at) VALUES (?1,?2,?3)",
                rusqlite::params![m.version, m.description, now_ms],
            )?;
            tx.commit()?;
        }
        Ok(())
    }
}
