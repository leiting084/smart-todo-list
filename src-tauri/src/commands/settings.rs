//! 设置命令：settings 表 KV 读写（主题、提醒开关等）。
//!
//! 这是**显式 KV 命令**（只能读写 settings 表的键值），不是通用 SQL 执行口（SPEC §6.2 IPC 红线）。
//! 注：dataDir / AI provider 等鸡生蛋配置走 exe 同级 app-config.json（SPEC §7.4），不在此模块。

use chrono::Local;
use rusqlite::params;
use tauri::State;

use crate::db::Db;

/// 读 settings 键；不存在返回 None。
#[tauri::command]
pub fn settings_get(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |r| r.get::<_, Option<String>>(0),
    )
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other.to_string()),
    })
}

/// 写 settings 键（upsert）。
#[tauri::command]
pub fn settings_set(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, Local::now().timestamp_millis()],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}
