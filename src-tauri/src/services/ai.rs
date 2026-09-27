//! AI 周报（F26 / T5.1）：自配 provider + SSE 流式 + 存为笔记。
//!
//! **只做自配轨**（不建云、不贴成本、数据不过第三方）。依据
//! `docs/research/external-benchmark-2026-09-15.md` §4.5：External 默认轨是把待办上传到作者服务器，
//! 我们不这么做——key 由用户自备，请求直连用户自己填的 base_url。
//!
//! 技术选型注释：
//! - 用 **reqwest blocking** 而非 async-openai。项目整体是同步 `Mutex<Connection>` 架构，
//!   引入 async 会让 `MutexGuard` 跨越 await 点，且无谓放大依赖树。
//! - SSE 流式用 blocking `Response` 的 `Read` 逐块读再按行解析，同样能逐字推给前端。
//! - TLS 选 rustls 而非 native-tls：避免 Windows 上编译 OpenSSL。

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::paths;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// 配置（存 app-config.json 的 `ai` 字段：不进 db、不进备份包、不进导出文件）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    /// OpenAI 兼容根地址，如 `https://api.deepseek.com`（需要 /v1 的服务请自行带上）
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}
impl Default for AiProvider {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "自定义".into(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AiConfig {
    pub providers: Vec<AiProvider>,
    pub current_provider_id: Option<String>,
    /// 用户编辑过的 system prompt；None = 用内置默认
    pub system_prompt: Option<String>,
}

/// 内置默认 system prompt（结构对标 External 产出：按日分组 + 问题 + 下周计划）。
pub fn default_prompt() -> String {
    "你是一名项目周报撰写助手。只根据给定的待办与备忘内容撰写周报，不回答任何与周报无关的问题，也不要编造未提供的内容。\n\
     输出结构：\n\
     1. 按日期分组，每天一个加粗小标题，把该日已完成事项扩写成通顺的工作描述；\n\
     2. 「遇到的问题」：从备忘与未完成事项中归纳，没有就写「无」；\n\
     3. 「下周计划」：基于未完成与逾期事项给出 3-5 条可执行计划。\n\
     使用简体中文，语气平实，不要使用表情符号。"
        .to_string()
}

pub fn load() -> AiConfig {
    paths::load_app_config()
        .ai
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &AiConfig) -> Result<(), String> {
    let mut app = paths::load_app_config();
    app.ai = Some(serde_json::to_value(cfg).map_err(|e| e.to_string())?);
    paths::save_app_config(&app).map_err(|e| format!("写入 app-config.json 失败：{e}"))
}

fn find<'a>(cfg: &'a AiConfig, id: &str) -> Result<&'a AiProvider, String> {
    let target = if id.is_empty() {
        cfg.current_provider_id.clone().unwrap_or_default()
    } else {
        id.to_string()
    };
    cfg.providers
        .iter()
        .find(|p| p.id == target)
        .ok_or_else(|| "未选择 provider（请先在设置里添加一个并设为当前）".to_string())
}

// ---------------------------------------------------------------------------
// 对外视图（key 打码，绝不明文出后端）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub has_key: bool,
    /// 形如 `sk-a****1b2c` 的提示串；无 key 为空串
    pub key_hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiConfigView {
    pub providers: Vec<ProviderView>,
    pub current_provider_id: Option<String>,
    pub system_prompt: String,
}

/// key 只留首尾各 4 位，中间一律打码。**日志与前端展示都不得出现明文 key。**
fn mask_key(key: &str) -> String {
    let n = key.chars().count();
    if n == 0 {
        return String::new();
    }
    if n <= 8 {
        return "****".to_string();
    }
    let head: String = key.chars().take(4).collect();
    let tail: String = key.chars().skip(n - 4).collect();
    format!("{head}****{tail}")
}

pub fn view(cfg: &AiConfig) -> AiConfigView {
    AiConfigView {
        providers: cfg
            .providers
            .iter()
            .map(|p| ProviderView {
                id: p.id.clone(),
                name: p.name.clone(),
                base_url: p.base_url.clone(),
                model: p.model.clone(),
                has_key: !p.api_key.is_empty(),
                key_hint: mask_key(&p.api_key),
            })
            .collect(),
        current_provider_id: cfg.current_provider_id.clone(),
        system_prompt: cfg.system_prompt.clone().unwrap_or_else(default_prompt),
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))
}

fn classify_http_error(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "请求超时（30 秒）".to_string()
    } else if e.is_connect() {
        format!("无法连接：{e}（检查网络、代理，或 base_url 是否需要带 /v1）")
    } else {
        format!("请求失败：{e}")
    }
}

fn classify_status(code: u16, body: &str) -> String {
    let tail: String = body.chars().take(160).collect();
    match code {
        401 | 403 => "API Key 无效或没有权限（401/403）".to_string(),
        404 => "地址不存在（404）——检查 base_url 是否正确、是否需要带 /v1".to_string(),
        429 => "请求过于频繁或额度不足（429）".to_string(),
        _ => format!("HTTP {code} {tail}"),
    }
}

/// T5.1.2 测试连接：GET `{base_url}/models`，返回前 20 个模型 id。
pub fn test_connection(cfg: &AiConfig, provider_id: &str) -> Result<Vec<String>, String> {
    let p = find(cfg, provider_id)?;
    if p.base_url.trim().is_empty() {
        return Err("base_url 未填写".into());
    }
    let url = format!("{}/models", p.base_url.trim_end_matches('/'));
    let resp = client()?
        .get(&url)
        .bearer_auth(&p.api_key)
        .send()
        .map_err(classify_http_error)?;
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let body = resp.text().unwrap_or_default();
        return Err(classify_status(code, &body));
    }
    let v: serde_json::Value = resp.json().map_err(|e| format!("响应不是合法 JSON：{e}"))?;
    let ids: Vec<String> = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .take(20)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return Err("接口通了但没取到模型列表（响应里没有 data[].id）".into());
    }
    Ok(ids)
}

/// 生成取消标志（T5.1.5：前端点"停止"置 true）。
pub static CANCEL: AtomicBool = AtomicBool::new(false);

/// T5.1.5 流式生成：`on_delta` 每收到一段就回调（用于 emit 给前端），返回全文。
pub fn generate<F>(cfg: &AiConfig, provider_id: &str, user_prompt: &str, on_delta: F) -> Result<String, String>
where
    F: Fn(&str) -> Result<(), String>,
{
    let p = find(cfg, provider_id)?;
    if p.base_url.trim().is_empty() {
        return Err("base_url 未填写".into());
    }
    if p.model.trim().is_empty() {
        return Err("未填写模型名（可先点测试连接拉取模型列表）".into());
    }

    let url = format!("{}/chat/completions", p.base_url.trim_end_matches('/'));
    let body = json!({
        "model": p.model,
        "stream": true,
        "messages": [
            { "role": "system", "content": cfg.system_prompt.clone().unwrap_or_else(default_prompt) },
            { "role": "user", "content": user_prompt }
        ]
    });

    let mut resp = client()?
        .post(&url)
        .bearer_auth(&p.api_key)
        .json(&body)
        .send()
        .map_err(classify_http_error)?;
    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let text = resp.text().unwrap_or_default();
        return Err(classify_status(code, &text));
    }

    let mut full = String::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 2048];

    loop {
        if CANCEL.load(Ordering::Relaxed) {
            return Err("已取消".into());
        }
        let n = resp.read(&mut chunk).map_err(|e| format!("读取响应流失败：{e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);

        // SSE：按行处理，只认 `data:` 前缀
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim();
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                return finish(full);
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
                if !delta.is_empty() {
                    full.push_str(delta);
                    on_delta(delta)?;
                }
            }
        }
    }
    finish(full)
}

fn finish(full: String) -> Result<String, String> {
    if full.trim().is_empty() {
        Err("未收到任何内容：模型可能不支持 stream，或响应不是 OpenAI 兼容格式".into())
    } else {
        Ok(full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_hides_middle() {
        assert_eq!(mask_key(""), "");
        assert_eq!(mask_key("short"), "****");
        let m = mask_key("sk-abcdefgh1234");
        assert!(m.starts_with("sk-a"), "{m}");
        assert!(m.ends_with("1234"), "{m}");
        assert!(m.contains("****"), "{m}");
        assert!(!m.contains("bcdefgh"), "中间段不得泄露：{m}");
    }

    #[test]
    fn view_never_exposes_plain_key() {
        let cfg = AiConfig {
            providers: vec![AiProvider {
                id: "p1".into(),
                name: "DeepSeek".into(),
                base_url: "https://api.deepseek.com".into(),
                api_key: "sk-super-secret-key-9999".into(),
                model: "deepseek-chat".into(),
            }],
            current_provider_id: Some("p1".into()),
            system_prompt: None,
        };
        let dumped = serde_json::to_string(&view(&cfg)).unwrap();
        assert!(!dumped.contains("super-secret-key"), "视图含明文 key：{dumped}");
    }

    #[test]
    fn default_prompt_used_when_unset() {
        assert_eq!(view(&AiConfig::default()).system_prompt, default_prompt());
    }

    #[test]
    fn find_falls_back_to_current_and_rejects_ghost() {
        assert!(find(&AiConfig::default(), "").is_err());
        let cfg = AiConfig {
            providers: vec![AiProvider { id: "a".into(), ..Default::default() }],
            current_provider_id: Some("a".into()),
            system_prompt: None,
        };
        assert!(find(&cfg, "").is_ok(), "空 id 应回落到 current");
        assert!(find(&cfg, "ghost").is_err());
    }

    #[test]
    fn status_classification_is_actionable() {
        assert!(classify_status(401, "").contains("Key"));
        assert!(classify_status(404, "").contains("/v1"));
        assert!(classify_status(429, "").contains("额度"));
        assert!(classify_status(500, "boom").contains("HTTP 500"));
    }

    #[test]
    fn cancel_flag_roundtrip() {
        CANCEL.store(false, Ordering::Relaxed);
        assert!(!CANCEL.load(Ordering::Relaxed));
        CANCEL.store(true, Ordering::Relaxed);
        assert!(CANCEL.load(Ordering::Relaxed));
        CANCEL.store(false, Ordering::Relaxed);
    }
}
