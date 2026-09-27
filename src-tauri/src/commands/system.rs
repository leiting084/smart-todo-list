//! 系统级命令：IPC 连通性自检、数据路径查询与迁移（F22）。

use serde::Serialize;
use tauri::AppHandle;

use crate::paths::{
    app_config_path, app_paths, copy_dir_recursive, load_app_config, resolve_data_root,
    root_dir, save_app_config,
};

/// IPC 连通性自检（T0.4 验收）。
#[tauri::command]
pub fn ping() -> String {
    "pong".to_string()
}

/// 返回当前数据根目录（F9 设置页显示）。
#[tauri::command]
pub fn app_data_dir() -> String {
    crate::paths::app_paths()
        .data_dir
        .to_string_lossy()
        .into_owned()
}

/// 数据目录信息（F22 设置页展示 + 迁移后回显）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataDirInfo {
    /// 当前生效的数据根目录
    pub data_dir: String,
    /// 应用安装根目录（app-config.json 所在）
    pub app_root: String,
    pub config_path: String,
    /// true = 来自 app-config.json 的自定义目录；false = 绿色默认（app 同级的 data/）
    pub is_custom: bool,
}

fn current_info() -> DataDirInfo {
    let cfg = load_app_config();
    DataDirInfo {
        data_dir: crate::paths::app_paths().data_dir.to_string_lossy().into_owned(),
        app_root: root_dir().to_string_lossy().into_owned(),
        config_path: app_config_path().to_string_lossy().into_owned(),
        is_custom: cfg.data_dir.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false),
    }
}

/// 查询当前数据目录信息（F22）。
#[tauri::command]
pub fn get_data_dir_info() -> DataDirInfo {
    current_info()
}

// ---------------------------------------------------------------------------
// F27 / T5.2 更新检查
// ---------------------------------------------------------------------------

/// 更新检查结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub has_update: bool,
    /// 新版本下载地址（更新源未提供时为空串）
    pub url: String,
}

/// 手动检查更新。
///
/// **决策（2026-09-20）**：绿色便携版**不做自动更新**——不引入 updater 插件、不做代码签名。
/// 便携版的更新方式本就是"下载新包覆盖 exe"，`data/` 在同级目录不受影响；
/// 而 updater 无签名时体验差（T5.2 原始调研结论），且会把"双击即用"的零依赖优势弄丢。
/// 因此这里只提供**手动检查**：读 `app-config.json` 的 `updateUrl`，
/// GET 期望返回 `{"version":"0.3.0","url":"https://…"}`。
#[tauri::command]
pub fn check_update() -> Result<UpdateInfo, String> {
    let source = load_app_config().update_url.unwrap_or_default();
    if source.trim().is_empty() {
        return Err(
            "未配置更新源（app-config.json 的 updateUrl 为空）。绿色便携版不做自动更新：\
             有新版本时下载新压缩包、覆盖 exe 即可，data/ 里的数据不受影响。"
                .into(),
        );
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;
    let resp = client
        .get(&source)
        .send()
        .map_err(|e| format!("检查更新失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("更新源返回 HTTP {}", resp.status().as_u16()));
    }
    let v: serde_json::Value = resp
        .json()
        .map_err(|e| format!("更新源返回的不是合法 JSON：{e}"))?;
    let latest = v["version"].as_str().unwrap_or_default().to_string();
    if latest.is_empty() {
        return Err("更新源响应里没有 version 字段".into());
    }
    let download = v["url"].as_str().unwrap_or_default().to_string();
    let current = env!("CARGO_PKG_VERSION").to_string();
    Ok(UpdateInfo { has_update: latest != current, current, latest, url: download })
}

/// 配置更新源地址（传空串 = 清除，不再检查）。
#[tauri::command]
pub fn set_update_url(url: String) -> Result<String, String> {
    let mut cfg = load_app_config();
    let t = url.trim().to_string();
    cfg.update_url = if t.is_empty() { None } else { Some(t) };
    save_app_config(&cfg).map_err(|e| format!("写入 app-config.json 失败：{e}"))?;
    Ok(load_app_config().update_url.unwrap_or_default())
}

/// 重启应用（F22 切换数据目录后必须重启才能生效）。
///
/// 不用 tauri 的 `restart()`（依赖不稳定特性）：直接拉起新进程再退出当前进程。
/// 注意顺序——**先 spawn 再 exit**，且新进程不带任何命令行参数。
#[tauri::command]
pub fn relaunch_app(app: AppHandle) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位自身路径：{e}"))?;
    std::process::Command::new(exe)
        .spawn()
        .map_err(|e| format!("拉起新进程失败：{e}"))?;
    app.exit(0);
    Ok(())
}

/// 切换数据目录（F22/T4.3）：把现有 data/ 迁移到新位置并写入 app-config.json。
///
/// 语义：
/// - 目标目录里已存在 `todolist.db` → **直接切换过去**（采用它的现有数据，不覆盖）；
/// - 否则把当前 data/ 整个拷过去，再写配置；
/// - 写完配置必须**重启**才生效（进程内 PATHS 是 OnceLock），由前端调用 relaunch。
#[tauri::command]
pub fn set_data_dir(target: Option<String>) -> Result<DataDirInfo, String> {
    use std::path::PathBuf;

    // target 为空 =「恢复默认」：删掉 dataDir 配置即可（原自定义数据保留在原地不动）。
    let new_root: Option<PathBuf> = match target.as_deref().map(str::trim) {
        Some("") | None => None,
        Some(p) => Some(PathBuf::from(p)),
    };

    // ⚠️ 必须基于现有配置改，不能新建 AppConfig：否则会把 ai / updateUrl 等
    // 同文件里的其它配置一起清掉（切换数据目录 → AI 配置丢失）。
    let mut new_cfg = crate::paths::load_app_config();
    new_cfg.data_dir = new_root.as_ref().map(|p| p.to_string_lossy().into_owned());
    let new_data_dir = new_root
        .as_ref()
        .map(|_| resolve_data_root(&root_dir(), &new_cfg))
        .unwrap_or_else(|| root_dir().join("data"));
    let current = app_paths().data_dir.clone();

    if new_data_dir == current {
        return Ok(current_info());
    }

    let new_db = new_data_dir.join("todolist.db");
    if new_db.is_file() {
        // 目标已有数据：采用它（不清空、不覆盖），仅切换配置。
        save_app_config(&new_cfg).map_err(|e| format!("写入 app-config.json 失败：{e}"))?;
        return Ok(current_info());
    }

    // 目标无数据：拷当前 data → 目标
    std::fs::create_dir_all(&new_data_dir)
        .map_err(|e| format!("无法创建目标目录 {}：{e}", new_data_dir.display()))?;
    if current.is_dir() {
        copy_dir_recursive(&current, &new_data_dir)
            .map_err(|e| format!("迁移数据失败：{e}"))?;
    }
    save_app_config(&new_cfg).map_err(|e| format!("写入 app-config.json 失败：{e}"))?;

    // 兜底校验：目标确实有库才算成功，否则回滚配置（避免"配置指向空目录、数据还留在旧处"）
    if !new_db.is_file() {
        let mut rollback = crate::paths::load_app_config();
        rollback.data_dir = Some(current.to_string_lossy().into_owned());
        let _ = save_app_config(&rollback);
        return Err(format!(
            "迁移后目标目录没有数据库 {}，已回滚配置",
            new_db.display()
        ));
    }
    Ok(current_info())
}

/// 今天（本地时区）YYYYMMDD。今日日期的唯一真相源，避免前端各算各的跨天不一致。
#[tauri::command]
pub fn today_date() -> i64 {
    crate::id::today_ymd()
}
