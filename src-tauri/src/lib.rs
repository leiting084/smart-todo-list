// 模块（SPEC §6.2）
mod commands;
mod db;
mod id;
mod models;
mod paths;
mod services;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use crate::db::Db;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 绿色路径初始化必须在任何窗口/WebView2 创建之前：
    // 建 data/{notes,backups,webview} 并在进程内设置 WEBVIEW2_USER_DATA_FOLDER（SPEC §7.3/§7.7）。
    let app_paths = paths::init().expect("数据目录初始化失败，无法启动");
    let webview_dir = app_paths.webview_dir.clone();
    let data_title = format!("智能待办清单 — data: {}", app_paths.data_dir.display());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // F19：自启时携带 `--autostart` 参数（第二个参数即追加到自启命令行的 args），
        // 启动时据此判断"是开机自启拉起来的" → 主窗不弹，只驻留托盘（与 External 行为一致）。
        // ⚠️ 该参数会写进注册表 Run 键，改这里后需要用户在托盘里重新开关一次自启才会生效。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .setup(move |app| {
            // F19：判定本次是否由开机自启拉起（tauri-plugin-autostart 追加 --autostart）。
            // 手动双击 exe 不含此参数 → 正常弹窗。
            let launched_by_autostart = std::env::args().any(|a| a == "--autostart");

            // T0.3：打开/迁移数据库并纳入 managed state（WAL + 版本化迁移 + 升级前备份）。
            // TODO(SPEC §7.5)：迁移失败后续改为弹错误对话框，而非 panic。
            let database = db::open(app_paths).expect("数据库打开/迁移失败");

            // T3.8：主窗位置/尺寸记忆（多显示器拔掉自动回主屏）——在 manage 前读取。
            let spec = crate::commands::window::main_window_spec(app.handle(), &database);

            // F15：启动即幂等生成"今天"的重复实例 + 自动顺延（否则要等跨零点线程那轮才有实例，
            // 用户当天上午打开"今天"会看不到重复待办）。best-effort，失败不阻断启动。
            if let Ok(conn) = database.0.lock() {
                let today_date = chrono::Local::now().date_naive();
                let _ = services::repeat::generate_instances(&conn, today_date);
                let _ = services::repeat::postpone_overdue(&conn, today_date);
            }

            // T3.3：托盘 + 全局快捷键（冲突时跳过注册不 panic）。
            services::tray::setup(app.handle(), &database)?;

            app.manage(database);

            // T3.2：浮窗**启动时预建并隐藏**（命令上下文建窗会卡死，已实测——勿移回命令内）。
            commands::window::create_mini(app, &app.state::<Db>()).ok();

            // T3.4：提醒后台扫描线程（20 秒/轮，notification_log 去重）。
            services::reminder::spawn(app.handle().clone());

            // 主窗口在代码中创建（tauri.conf.json 的 windows 留空），
            // 以便显式指定 data_directory：否则 Tauri 会在 Windows 强制创建
            // %LOCALAPPDATA%\<identifier> 作为默认 webview 数据目录（tauri-2.11 manager/webview.rs）。
            // 显式指定后，EBWebView 缓存只落 data/webview，AppData 不产生任何目录。
            // 首帧标题即含数据路径；前端挂载后还会以 app_data_dir 命令结果刷新 document.title。
            // `mut` 仅在 debug 构建下被用到（下方 cfg(debug_assertions) 追加 CDP 参数）；
            // release 构建该分支消失，clippy 会报 unused_mut，故在此显式放行。
            #[allow(unused_mut)]
            let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title(&data_title)
                .inner_size(spec.width, spec.height)
                .position(spec.x, spec.y)
                .min_inner_size(720.0, 520.0)
                // F19：开机自启拉起时**不弹主窗**，只驻留托盘（SPEC F19 明确要求）。
                // 窗口仍需创建——前端要挂载、提醒线程要工作，只是不显示。
                .visible(!launched_by_autostart)
                .data_directory(webview_dir.clone());

            // WebView2 默认会注册原生文件拖放目标（Tauri dragDropEnabled=true），
            // 它会吞掉页面内的 HTML5 拖拽事件 → 待办列表拖不动、无法换序。
            // 关掉原生拖放，把拖拽交回前端（本应用不依赖"把文件拖到窗口"导入，导入均走对话框）。
            // 该 builder 方法仅 Windows 存在，故 cfg 保护以保证跨平台（含开源版）可编译。
            #[cfg(windows)]
            {
                builder = builder.drag_and_drop(false);
            }

            // 仅 dev：开 WebView2 远程调试端口，供 Playwright CDP 做真实 IPC 端到端验证。
            // release 不含此参数。
            #[cfg(debug_assertions)]
            {
                builder = builder.additional_browser_args("--remote-debugging-port=9222");
            }

            builder.build()?;

            // F19：自启静默驻留时给一条提示，否则用户会以为"没启动"（托盘图标在隐藏区，不一定看得见）。
            // 延迟 3 秒：开机瞬间通知子系统可能还没就绪。
            if launched_by_autostart {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let _ = tauri_plugin_notification::NotificationExt::notification(&handle)
                        .builder()
                        .title("智能待办清单")
                        .body("已在后台运行 · Ctrl+Alt+T 呼出主窗")
                        .show();
                });
            }
            Ok(())
        })
        // T3.3：主窗"关闭"=隐藏驻留托盘（退出走托盘菜单）；浮窗关闭不受影响
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // 系统/设置
            commands::system::ping,
            commands::system::app_data_dir,
            commands::ai::ai_get_config,
            commands::ai::ai_save_providers,
            commands::ai::ai_test_connection,
            commands::ai::ai_set_prompt,
            commands::ai::ai_generate,
            commands::ai::ai_cancel,
            commands::system::check_update,
            commands::system::set_update_url,
            commands::system::today_date,
            commands::system::get_data_dir_info,
            commands::system::set_data_dir,
            commands::system::relaunch_app,
            commands::settings::settings_get,
            commands::settings::settings_set,
            // Todo（T1.1/T1.2/T1.4/T3.1）
            commands::todo::todo_create,
            commands::todo::todo_get,
            commands::todo::todo_update,
            commands::todo::todo_delete,
            commands::todo::todo_set_completed,
            commands::todo::todo_list,
            commands::todo::todo_reorder,
            commands::todo::todo_complete_with_children,
            commands::todo::todos_bulk_create,
            commands::todo::todo_batch_update,
            commands::todo::todo_batch_delete,
            // 标签（T1.3）
            commands::tag::tag_create,
            commands::tag::tag_rename,
            commands::tag::tag_delete,
            commands::tag::item_set_tags,
            commands::tag::tags_list,
            commands::tag::items_by_tag,
            // 项目（T1.5）
            commands::project::project_create,
            commands::project::project_update,
            commands::project::project_archive,
            commands::project::project_delete,
            commands::project::project_list,
            commands::project::project_get_stats,
            // 人员与协作完成（T1.6）
            commands::person::person_create,
            commands::person::person_update,
            commands::person::person_archive,
            commands::person::person_list,
            commands::person::todo_get_assignees,
            commands::person::todo_set_assignees,
            commands::person::todo_person_toggle,
            // 长期目标（T1.7）
            commands::goal::goal_create,
            commands::goal::goal_update,
            commands::goal::goal_delete,
            commands::goal::goal_list,
            commands::goal::goal_set_links,
            commands::goal::goal_get_links,
            commands::goal::goal_get_stats,
            // 仪表板（T1.8）
            commands::dashboard::dashboard_get,
            // 月总览（T3.6）
            commands::dashboard::overview_month,
            commands::dashboard::overview_batch_complete,
            commands::dashboard::overview_batch_uncomplete,
            // 浮窗（T3.2）
            commands::window::mini_window_toggle,
            commands::window::window_save_bounds,
            commands::window::mini_window_get_pos,
            commands::window::mini_window_get_size,
            commands::window::mini_get_settings,
            commands::window::mini_set_pin_mode,
            commands::window::mini_set_opacity,
            commands::window::mini_set_edge_hide,
            commands::window::mini_edge_collapse,
            commands::window::mini_edge_expand,
            // 托盘快捷键（T3.3）
            services::tray::get_shortcuts,
            // 提醒（T3.4）
            services::reminder::reminders_get_enabled,
            services::reminder::reminders_set_enabled,
            // 重复待办（T3.5）
            services::repeat::repeat_set,
            services::repeat::repeat_get,
            services::repeat::repeat_clear,
            services::repeat::repeat_generate,
            services::repeat::repeat_postpone,
            // 链接查询（T2.1/T2.4）
            commands::link::links_for_entity,
            // 备忘（T2.1）
            commands::memo::memo_create,
            commands::memo::memo_update,
            commands::memo::memo_delete,
            commands::memo::memo_list,
            commands::memo::memo_create_from_todo_cmd,
            commands::memo::memo_to_note_cmd,
            // 笔记（T2.2）
            commands::note::note_create,
            commands::note::note_rename,
            commands::note::note_delete,
            commands::note::note_list,
            commands::note::note_get,
            commands::note::note_get_content,
            commands::note::note_save_content,
            commands::note::note_create_todo_cmd,
            // 旧版 JSON 导入（T1.9）
            services::importer::import_legacy_json,
            // 导出（T4.1）
            services::exporter::export_todos_markdown,
            services::exporter::export_todos_csv,
            services::exporter::export_todos_html,
            // 全文搜索（T4.5）
            services::search::search_all,
            // 最近操作只读日志（#32）
            services::oplog::oplog_list,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // T3.2 验收：主窗关闭后浮窗存活——
            // 所有窗口都关闭触发 ExitRequested（code=None 表示非显式退出）；
            // 浮窗还活着就不退出（此时主窗已销毁，只剩浮窗）。
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() && commands::window::prevent_exit_when_mini_alive(app) {
                    api.prevent_exit();
                }
            }
        });
}
