//! AI 周报命令（F26 / T5.1）。
//!
//! 命令层只做三件事：读写配置、跑测试连接、把生成任务丢到后台线程并 emit 流。
//! 生成必须走线程——blocking HTTP 会阻塞，直接放在命令里会卡死 UI。

use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter};

use crate::services::ai;

/// 读取配置（key 已打码）。
#[tauri::command]
pub fn ai_get_config() -> ai::AiConfigView {
    ai::view(&ai::load())
}

/// 保存 provider 列表与当前选择。
///
/// ⚠️ 前端传回的 `api_key` 为空串时**保留原 key**（只改名字/base_url/model 的场景），
/// 否则用户每次编辑别的字段都会把 key 清掉。要清空 key 需显式传一个空白占位符（不提供该能力）。
#[tauri::command]
pub fn ai_save_providers(
    providers: Vec<ai::AiProvider>,
    current_provider_id: Option<String>,
) -> Result<ai::AiConfigView, String> {
    let mut cfg = ai::load();
    let mut next = providers;
    for p in next.iter_mut() {
        if p.api_key.is_empty() {
            if let Some(old) = cfg.providers.iter().find(|o| o.id == p.id) {
                p.api_key = old.api_key.clone();
            }
        }
    }
    cfg.providers = next;
    cfg.current_provider_id = current_provider_id;
    // 当前 provider 被删 → 清空 current，避免悬空引用（T5.1.1 验收项）
    if let Some(id) = &cfg.current_provider_id {
        if !cfg.providers.iter().any(|p| p.id == *id) {
            cfg.current_provider_id = None;
        }
    }
    ai::save(&cfg)?;
    Ok(ai::view(&cfg))
}

/// T5.1.2 测试连接：返回可用模型 id（前 20 个），错误分类提示。
#[tauri::command]
pub fn ai_test_connection(provider_id: String) -> Result<Vec<String>, String> {
    ai::test_connection(&ai::load(), &provider_id)
}

/// T5.1.4 保存自定义 system prompt；传空串 = 恢复内置默认。
#[tauri::command]
pub fn ai_set_prompt(prompt: String) -> Result<ai::AiConfigView, String> {
    let mut cfg = ai::load();
    let t = prompt.trim().to_string();
    cfg.system_prompt = if t.is_empty() { None } else { Some(t) };
    ai::save(&cfg)?;
    Ok(ai::view(&cfg))
}

/// 流式生成：立即返回，结果通过事件推送。
/// - `ai://chunk` 每段文本
/// - `ai://done`   全文
/// - `ai://error`  错误信息
#[tauri::command]
pub fn ai_generate(app: AppHandle, provider_id: String, user_prompt: String) -> Result<(), String> {
    ai::CANCEL.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        let cfg = ai::load();
        let handle = app.clone();
        let r = ai::generate(&cfg, &provider_id, &user_prompt, |delta| {
            handle
                .emit("ai://chunk", delta.to_string())
                .map_err(|e| e.to_string())
        });
        match r {
            Ok(full) => {
                let _ = app.emit("ai://done", full);
            }
            Err(e) => {
                let _ = app.emit("ai://error", e);
            }
        }
    });
    Ok(())
}

/// 中断生成。
#[tauri::command]
pub fn ai_cancel() -> Result<(), String> {
    ai::CANCEL.store(true, Ordering::Relaxed);
    Ok(())
}
