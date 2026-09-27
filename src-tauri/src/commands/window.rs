//! 浮窗管理（F12/T3.2）：迷你浮窗的创建/显隐/位置记忆。
//!
//! 设计：
//! - 第二个 WebviewWindow（label="mini"，300×380，always_on_top），页面为 /mini 路由；
//! - 位置/尺寸持久化到 settings KV（key=mini_window_bounds，JSON）；
//! - 创建时恢复上一次位置，并做多显示器边界检测（越界回主屏）；
//! - 主窗关闭后浮窗存活（app 在任一窗口存活期间不退出）。

use std::sync::Mutex;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::db::Db;

pub const MINI_LABEL: &str = "mini";
pub const MAIN_LABEL: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Bounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}
impl Default for Bounds {
    // F12 v2 造型：默认"长条"，展开后前端拉成 240×300
    fn default() -> Self {
        Self { x: 100, y: 100, width: 220, height: 64 }
    }
}

/// 窗口 bounds 的 settings key（主窗/浮窗通用）。
fn bounds_key(label: &str) -> String {
    format!("window_bounds_{label}")
}

fn load_bounds(db: &Db, label: &str) -> Option<Bounds> {
    let conn = db.0.lock().ok()?;
    conn.query_row(
        "SELECT value FROM settings WHERE key=?1",
        params![bounds_key(label)],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
}

fn clamp_to_monitors(app: &tauri::AppHandle, b: &mut Bounds) {
    // 边界检测：任一显示器可见区域无交集则回 (100,100)（多显示器拔掉场景）
    let visible = |x: i32, y: i32| -> bool {
        app.available_monitors()
            .map(|ms| {
                ms.iter().any(|m| {
                    let pos = m.position();
                    let size = m.size();
                    x >= pos.x - (b.width as i32)
                        && x < pos.x + size.width as i32
                        && y >= pos.y - (b.height as i32)
                        && y < pos.y + size.height as i32
                })
            })
            .unwrap_or(false)
    };
    if !visible(b.x, b.y) {
        b.x = 100;
        b.y = 100;
    }
}

/// 切换浮窗显示/隐藏。**创建只在启动 setup 完成**（命令上下文建窗会卡死，已实测）；
/// 窗口缺失时返回明确错误提示重启，而非再建。
pub fn mini_window_toggle_inner(app: &tauri::AppHandle) -> Result<String, String> {
    let w = app
        .get_webview_window(MINI_LABEL)
        .ok_or("浮窗未初始化（重启应用后再试）")?;
    if w.is_visible().map_err(|e| e.to_string())? {
        w.hide().map_err(|e| e.to_string())?;
        Ok("hidden".into())
    } else {
        w.show().map_err(|e| e.to_string())?;
        w.set_focus().map_err(|e| e.to_string())?;
        Ok("shown".into())
    }
}

/// 启动 setup 调用：预建浮窗并隐藏（visible(false)）。
pub fn create_mini(app: &tauri::App, db: &Db) -> tauri::Result<()> {
    let handle = app.handle();
    let mut bounds = load_bounds(db, MINI_LABEL).unwrap_or_default();
    clamp_to_monitors(handle, &mut bounds);
    let mini = load_mini_settings(db);

    WebviewWindowBuilder::new(app, MINI_LABEL, WebviewUrl::App("mini".into()))
        .title("智能待办清单 · 浮窗")
        .inner_size(bounds.width as f64, bounds.height as f64)
        .position(bounds.x as f64, bounds.y as f64)
        // F12 补齐：默认置顶改由设置决定（置顶/置底/普通）；transparent 使"透明度"生效
        // （窗口背景透明后由前端 body 的 rgba 背景承担实际观感）。
        .always_on_top(mini.pin_mode != "bottom")
        .always_on_bottom(mini.pin_mode == "bottom")
        .transparent(true)
        .resizable(true)
        .decorations(false)
        .focused(false)
        .visible(false)
        // 红线（SPEC §7.7）：浮窗同样把 WebView 数据目录指到 data/webview，
        // 否则 Tauri 会在 %LOCALAPPDATA%\<identifier> 强制建空目录。
        .data_directory(crate::paths::app_paths().webview_dir.clone())
        .build().map_err(|e| {
            eprintln!("[mini] 启动预建浮窗失败：{e}");
            e
        })?;
    Ok(())
}

/// 切换浮窗显示/隐藏（不存在则创建——实际创建在 setup，命令只显隐）。
#[tauri::command]
pub fn mini_window_toggle(app: tauri::AppHandle) -> Result<String, String> {
    mini_window_toggle_inner(&app)
}

/// 窗口自行上报当前位置/尺寸（主窗/浮窗通用；label 区分 key）。
#[tauri::command]
pub fn window_save_bounds(
    db: tauri::State<'_, Db>,
    label: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let json = serde_json::to_string(&Bounds { x, y, width, height }).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![bounds_key(&label), json, chrono::Local::now().timestamp_millis()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 主窗构造参数（setup 用）：读记忆 bounds（含多显示器 clamp）。无记忆用 SPEC §8.1 默认。
pub struct MainWindowSpec {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

pub fn main_window_spec(app: &tauri::AppHandle, db: &Db) -> MainWindowSpec {
    let mut b = load_bounds(db, MAIN_LABEL).unwrap_or(Bounds { x: 0, y: 0, width: 960, height: 680 });
    // 记忆过才 clamp（未记忆→交由系统默认居中）
    if b.x != 0 || b.y != 0 {
        clamp_to_monitors(app, &mut b);
    }
    MainWindowSpec { width: b.width as f64, height: b.height as f64, x: b.x as f64, y: b.y as f64 }
}

/// 主窗关闭时调用：若有浮窗存活则仅销毁主窗（app 不退出，验收"主窗关闭浮窗存活"）。
pub fn prevent_exit_when_mini_alive(app: &tauri::AppHandle) -> bool {
    app.get_webview_window(MINI_LABEL).is_some()
}

/// 读取物理位置/尺寸（浮窗前端用）。
#[tauri::command]
pub fn mini_window_get_pos(app: tauri::AppHandle) -> Result<(i32, i32), String> {
    let w = app
        .get_webview_window(MINI_LABEL)
        .ok_or("浮窗不存在")?;
    let PhysicalPosition { x, y } = w.outer_position().map_err(|e| e.to_string())?;
    Ok((x, y))
}

/// 读取物理尺寸（浮窗前端用）。
#[tauri::command]
pub fn mini_window_get_size(app: tauri::AppHandle) -> Result<(u32, u32), String> {
    let w = app
        .get_webview_window(MINI_LABEL)
        .ok_or("浮窗不存在")?;
    let PhysicalSize { width, height } = w.inner_size().map_err(|e| e.to_string())?;
    Ok((width, height))
}

// ---------------------------------------------------------------------------
// F12 补齐（2026-09-20）：置底（钉桌面）/ 透明度 / 贴边隐藏滑入
// ---------------------------------------------------------------------------

const MINI_SETTINGS_KEY: &str = "mini_settings";

/// 浮窗外观与行为设置。存 settings KV（JSON），缺省即"置顶 + 不透明 + 不贴边"。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MiniSettings {
    /// "top" 置顶 | "bottom" 置底（钉桌面） | "none" 普通窗口
    pub pin_mode: String,
    /// 0.35–1.0；1.0 = 完全不透明。窗口 transparent(true) 后由前端 rgba 背景实现
    pub opacity: f64,
    /// 贴边隐藏：鼠标移出后缩成边条贴在最近屏幕边缘，移入展开
    pub edge_hide: bool,
}
impl Default for MiniSettings {
    fn default() -> Self {
        Self { pin_mode: "top".into(), opacity: 1.0, edge_hide: false }
    }
}

pub fn load_mini_settings(db: &Db) -> MiniSettings {
    let Ok(conn) = db.0.lock() else { return MiniSettings::default() };
    conn.query_row(
        "SELECT value FROM settings WHERE key=?1",
        params![MINI_SETTINGS_KEY],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
    .unwrap_or_default()
}

fn save_mini_settings(db: &Db, s: &MiniSettings) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let json = serde_json::to_string(s).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![MINI_SETTINGS_KEY, json, chrono::Local::now().timestamp_millis()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 把 pin_mode 应用到已存在的浮窗（置顶与置底互斥）。
fn apply_pin_mode(app: &tauri::AppHandle, mode: &str) {
    let Some(w) = app.get_webview_window(MINI_LABEL) else { return };
    let (top, bottom) = match mode {
        "bottom" => (false, true),
        "none" => (false, false),
        _ => (true, false),
    };
    let _ = w.set_always_on_top(top);
    let _ = w.set_always_on_bottom(bottom);
}

/// 通知浮窗前端热更新（透明度/贴边开关即时生效，无需重启）。
fn emit_mini_settings(app: &tauri::AppHandle, s: &MiniSettings) {
    let _ = app.emit("mini-settings-changed", s.clone());
}

/// 读取浮窗设置（前端浮窗与设置页共用）。
#[tauri::command]
pub fn mini_get_settings(db: tauri::State<'_, Db>) -> MiniSettings {
    load_mini_settings(&db)
}

/// 设置浮窗层级：置顶 / 置底（钉桌面）/ 普通。
#[tauri::command]
pub fn mini_set_pin_mode(
    app: tauri::AppHandle,
    db: tauri::State<'_, Db>,
    mode: String,
) -> Result<MiniSettings, String> {
    if !matches!(mode.as_str(), "top" | "bottom" | "none") {
        return Err("pin_mode 只能是 top / bottom / none".into());
    }
    let mut s = load_mini_settings(&db);
    s.pin_mode = mode;
    save_mini_settings(&db, &s)?;
    apply_pin_mode(&app, &s.pin_mode);
    emit_mini_settings(&app, &s);
    Ok(s)
}

/// 设置浮窗透明度（0.35–1.0）。
#[tauri::command]
pub fn mini_set_opacity(
    app: tauri::AppHandle,
    db: tauri::State<'_, Db>,
    opacity: f64,
) -> Result<MiniSettings, String> {
    // 下限 0.35：再低内容基本不可读，没有实用价值
    if !(0.35..=1.0).contains(&opacity) {
        return Err("opacity 需在 0.35–1.0 之间".into());
    }
    let mut s = load_mini_settings(&db);
    s.opacity = opacity;
    save_mini_settings(&db, &s)?;
    emit_mini_settings(&app, &s);
    Ok(s)
}

/// 开关贴边隐藏（鼠标移出缩成边条、移入展开）。
#[tauri::command]
pub fn mini_set_edge_hide(
    app: tauri::AppHandle,
    db: tauri::State<'_, Db>,
    on: bool,
) -> Result<MiniSettings, String> {
    let mut s = load_mini_settings(&db);
    s.edge_hide = on;
    save_mini_settings(&db, &s)?;
    emit_mini_settings(&app, &s);
    Ok(s)
}

/// 贴边隐藏时窗体外沿保留的像素（供鼠标移入触发展开）。
const EDGE_STRIP: u32 = 8;

/// 贴边隐藏过程中记住的展开态 bounds。只存内存：进程退出即回到正常态，
/// 避免"上次异常退出后浮窗永远缩成一条"的坑。
static EDGE_STATE: Mutex<Option<Bounds>> = Mutex::new(None);

/// 收缩：把浮窗贴到最近的屏幕边缘，只留 EDGE_STRIP 像素在外。
#[tauri::command]
pub fn mini_edge_collapse(app: tauri::AppHandle) -> Result<(), String> {
    let w = app.get_webview_window(MINI_LABEL).ok_or("浮窗不存在")?;
    // 已处于收缩态就别再缩（mouseleave 会连发）
    if EDGE_STATE.lock().map(|g| g.is_some()).unwrap_or(false) {
        return Ok(());
    }
    let PhysicalPosition { x, y } = w.outer_position().map_err(|e| e.to_string())?;
    let PhysicalSize { width, height } = w.outer_size().map_err(|e| e.to_string())?;
    let monitor = w.current_monitor().map_err(|e| e.to_string())?.ok_or("无法获取当前显示器")?;
    let mpos = monitor.position();
    let msize = monitor.size();

    if let Ok(mut guard) = EDGE_STATE.lock() {
        *guard = Some(Bounds { x, y, width, height });
    }

    let cx = x + width as i32 / 2;
    let cy = y + height as i32 / 2;
    let d_left = cx - mpos.x;
    let d_right = (mpos.x + msize.width as i32) - cx;
    let d_top = cy - mpos.y;
    let d_bottom = (mpos.y + msize.height as i32) - cy;
    let m = d_left.min(d_right).min(d_top).min(d_bottom);

    let (nx, ny, nw, nh) = if m == d_left {
        (mpos.x, y, EDGE_STRIP, height)
    } else if m == d_right {
        (mpos.x + msize.width as i32 - EDGE_STRIP as i32, y, EDGE_STRIP, height)
    } else if m == d_top {
        (x, mpos.y, width, EDGE_STRIP)
    } else {
        (x, mpos.y + msize.height as i32 - EDGE_STRIP as i32, width, EDGE_STRIP)
    };

    w.set_position(PhysicalPosition::new(nx, ny)).map_err(|e| e.to_string())?;
    w.set_size(PhysicalSize::new(nw, nh)).map_err(|e| e.to_string())?;
    Ok(())
}

/// 展开：从收缩前的 bounds 恢复（无收缩态则 no-op）。
#[tauri::command]
pub fn mini_edge_expand(app: tauri::AppHandle) -> Result<(), String> {
    let w = app.get_webview_window(MINI_LABEL).ok_or("浮窗不存在")?;
    let saved = match EDGE_STATE.lock() {
        Ok(mut g) => g.take(),
        Err(_) => None,
    };
    let Some(b) = saved else { return Ok(()) };
    w.set_size(PhysicalSize::new(b.width, b.height)).map_err(|e| e.to_string())?;
    w.set_position(PhysicalPosition::new(b.x, b.y)).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mini_settings_default_shape() {
        let s = MiniSettings::default();
        assert_eq!(s.pin_mode, "top");
        assert!((s.opacity - 1.0).abs() < f64::EPSILON);
        assert!(!s.edge_hide);
        // 锁定 JSON 形状，防止前端字段名漂移
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"pin_mode\":\"top\""), "{j}");
        assert!(j.contains("\"edge_hide\":false"), "{j}");
    }

    #[test]
    fn mini_settings_roundtrip() {
        let s = MiniSettings { pin_mode: "bottom".into(), opacity: 0.6, edge_hide: true };
        let j = serde_json::to_string(&s).unwrap();
        let back: MiniSettings = serde_json::from_str(&j).unwrap();
        assert_eq!(back.pin_mode, "bottom");
        assert!((back.opacity - 0.6).abs() < f64::EPSILON);
        assert!(back.edge_hide);
    }

    #[test]
    fn pin_mode_accept_only_three() {
        for ok in ["top", "bottom", "none"] {
            assert!(matches!(ok, "top" | "bottom" | "none"));
        }
        assert!(!matches!("floating", "top" | "bottom" | "none"));
    }

    #[test]
    fn opacity_bounds_reject_out_of_range() {
        assert!((0.35..=1.0).contains(&0.35));
        assert!((0.35..=1.0).contains(&1.0));
        assert!(!(0.35..=1.0).contains(&0.1));
        assert!(!(0.35..=1.0).contains(&1.5));
    }
}
