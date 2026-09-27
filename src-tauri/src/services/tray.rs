//! 系统托盘 + 全局快捷键（F13/T3.3，SPEC T3.3）。
//!
//! - 托盘菜单：显示主窗 / 开关浮窗 / 退出；左键单击托盘 = 显示主窗；
//! - 主窗"关闭"= 隐藏驻留托盘（SPEC"关窗=驻留"）；退出走托盘菜单；
//! - 全局快捷键：呼出主窗（默认 Ctrl+Alt+T）、开关浮窗（默认 Ctrl+Alt+N），
//!   键位存 settings KV（shortcut_show_main / shortcut_toggle_mini），启动时读取注册；
//!   **注册失败（冲突）→ 跳过该键并打日志，不 panic（TASKS 验收：冲突给提示不注册）**。

use rusqlite::{params, Connection};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::db::Db;

fn settings_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| {
        r.get::<_, Option<String>>(0)
    })
    .ok()
    .flatten()
}

fn db_get(db: &Db, key: &str) -> Option<String> {
    let conn = db.0.lock().ok()?;
    settings_get(&conn, key)
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn toggle_mini(app: &AppHandle) {
    let _ = crate::commands::window::mini_window_toggle_inner(app);
}

/// 在托盘/快捷键两个入口复用的主窗"关闭=隐藏"由前端/窗口事件处理，这里只管托盘 UI。
pub fn setup(app: &AppHandle, db: &Db) -> Result<(), Box<dyn std::error::Error>> {
    // ---- 托盘 ----
    let show = MenuItem::with_id(app, "show", "显示主窗", true, None::<&str>)?;
    let mini = MenuItem::with_id(app, "mini", "开关浮窗", true, None::<&str>)?;
    // T3.8：开机自启开关（tauri-plugin-autostart）
    let autostart_enabled = tauri_plugin_autostart::ManagerExt::autolaunch(app).is_enabled().unwrap_or(false);
    let auto = MenuItem::with_id(
        app,
        "autostart",
        if autostart_enabled { "✓ 开机自启" } else { "开机自启" },
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &mini, &auto, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("默认图标缺失").clone())
        .tooltip("智能待办清单")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "mini" => toggle_mini(app),
            "autostart" => {
                use tauri_plugin_autostart::ManagerExt;
                let on = app.autolaunch().is_enabled().unwrap_or(false);
                let result = if on {
                    app.autolaunch().disable()
                } else {
                    app.autolaunch().enable()
                };
                if let Err(e) = result {
                    eprintln!("[autostart] 切换失败：{e}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击托盘 = 显示主窗
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    let _ = db; // db 仅快捷键段用（下）

    // ---- 全局快捷键 ----
    let defaults: [(&str, &str); 2] = [
        ("shortcut_show_main", "ctrl+alt+t"),
        ("shortcut_toggle_mini", "ctrl+alt+n"),
    ];
    let gs = app.global_shortcut();
    for (key, default_sc) in defaults {
        let sc_str = db_get(db, key).unwrap_or_else(|| default_sc.to_string());
        let shortcut: Shortcut = match sc_str.parse() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[shortcut] `{key}` 键位无法解析（{sc_str}）：{e}，跳过注册");
                continue;
            }
        };
        let key_owned = key.to_string();
        let reg = gs.on_shortcut(shortcut, move |app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                match key_owned.as_str() {
                    "shortcut_show_main" => show_main(app),
                    "shortcut_toggle_mini" => toggle_mini(app),
                    _ => {}
                }
            }
        });
        if let Err(e) = reg {
            // TASKS 验收：快捷键冲突时给出提示并不注册（跳过，不 panic）
            eprintln!("[shortcut] `{key}` 注册失败（可能与其他程序冲突）：{e}");
        }
    }
    Ok(())
}

/// 设置页改键后读取当前键位（前端展示用）。
#[tauri::command]
pub fn get_shortcuts(db: tauri::State<'_, Db>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "showMain": db_get(&db, "shortcut_show_main").unwrap_or_else(|| "ctrl+alt+t".into()),
        "toggleMini": db_get(&db, "shortcut_toggle_mini").unwrap_or_else(|| "ctrl+alt+n".into()),
    }))
}
