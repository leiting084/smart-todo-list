//! 绿色数据路径（SPEC §7 最高优先级红线）。
//!
//! 全仓库**唯一**允许计算"数据放哪"的模块：
//! - release：`exe 同级/data`（文件夹拷走即用）；
//! - dev（debug_assertions）：工程根 `data/`（`CARGO_MANIFEST_DIR` = .../src-tauri，上一级即工程根）。
//!
//! 红线：
//! - 禁止 setx / 注册表 Environment / 修改 PATH·USERPROFILE·TEMP·TMP·APPDATA（旧版 47GB 污染教训）；
//! - 唯一允许的进程内环境变量是 `WEBVIEW2_USER_DATA_FOLDER`（把 EBWebView 缓存关进 data/webview，SPEC §7.7），
//!   且必须在创建任何窗口之前设置；
//! - 所有路径在 Rust 侧拼好再传给前端，前端不自拼绝对路径（SPEC §7.6）。

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 应用根级别配置（`<root>/app-config.json`，SPEC §7.4：不能进 DB 的鸡生蛋配置）。
///
/// 保留 JSON 形状扁平 + camelCase，后续 T5.1 AI provider 等继续往里加字段。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// 自定义数据根目录（含结尾 `data`）。`None` = 绿色默认路径（`<root>/data`）。
    #[serde(rename = "dataDir", default, skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<String>,
    /// AI 周报配置（F26：providers / currentProviderId / systemPrompt）。
    /// 用裸 JSON 值而非具体类型：paths.rs 不依赖 services 层的类型定义，
    /// 由 `services::ai` 负责序列化/反序列化。**不进 db、不进 backups、不进导出包。**
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<serde_json::Value>,
    /// 更新检查源（F27）。留空 = 未配置，检查更新时提示而非静默失败。
    #[serde(rename = "updateUrl", default, skip_serializing_if = "Option::is_none")]
    pub update_url: Option<String>,
}

/// 读 app-config.json；不存在/损坏一律回落空配置（不阻塞启动）。
pub fn load_app_config() -> AppConfig {
    match fs::read_to_string(app_config_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

/// 写 app-config.json。
pub fn save_app_config(cfg: &AppConfig) -> io::Result<()> {
    let json =
        serde_json::to_string_pretty(cfg).map_err(io::Error::other)?;
    fs::write(app_config_path(), json)
}

/// 纯函数：给定 root 与配置，算出数据根目录（可单测，不碰磁盘）。
///
/// 规则：`dataDir` 非空时用它（用户选的目录若不以 `data` 结尾，自动补一级 `data`，
/// 避免把 db/notes/backups 散到用户所选目录本身）；否则绿色默认 `<root>/data`。
pub(crate) fn resolve_data_root(root: &Path, cfg: &AppConfig) -> PathBuf {
    if let Some(custom) = cfg.data_dir.as_deref() {
        let c = custom.trim();
        if !c.is_empty() {
            let p = PathBuf::from(c);
            return if p.file_name().and_then(|n| n.to_str()) == Some("data") {
                p
            } else {
                p.join("data")
            };
        }
    }
    root.join("data")
}

/// 应用根目录（data/ 与 app-config.json 的共同父目录）。
pub fn root_dir() -> PathBuf {
    if cfg!(debug_assertions) {
        // 编译期固定到 src-tauri，上一级即工程根；比依赖运行时 cwd 可靠。
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    } else {
        let exe = std::env::current_exe().expect("无法定位当前可执行文件");
        exe.parent()
            .expect("可执行文件没有父目录")
            .to_path_buf()
    }
}

/// 数据根目录 `<root>/data`（F22：`app-config.json` 的 `dataDir` 优先）。
pub fn data_dir() -> PathBuf {
    resolve_data_root(&root_dir(), &load_app_config())
}

/// 库外配置文件 `<root>/app-config.json`（SPEC §7.4：dataDir/AI provider 等不能存进 DB，鸡生蛋问题）。
// F22 自定义数据目录 / T5.1 AI provider 配置时消费。
#[allow(dead_code)]
pub fn app_config_path() -> PathBuf {
    root_dir().join("app-config.json")
}

/// 数据目录内部布局（纯函数，便于单测）。
// 部分字段（db_path/notes_dir 等）在 T0.3 数据层与 T2.2 笔记文件存储才消费，先完整表达布局。
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub notes_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub webview_dir: PathBuf,
    pub db_path: PathBuf,
}

fn layout(data_dir: &Path) -> AppPaths {
    AppPaths {
        data_dir: data_dir.to_path_buf(),
        notes_dir: data_dir.join("notes"),
        backups_dir: data_dir.join("backups"),
        webview_dir: data_dir.join("webview"),
        db_path: data_dir.join("todolist.db"),
    }
}

/// 在给定数据根下创建全部子目录并返回布局（可对临时目录做单测）。
fn materialize(data_dir: &Path) -> io::Result<AppPaths> {
    let p = layout(data_dir);
    fs::create_dir_all(&p.data_dir)?;
    fs::create_dir_all(&p.notes_dir)?;
    fs::create_dir_all(&p.backups_dir)?;
    fs::create_dir_all(&p.webview_dir)?;
    Ok(p)
}

/// 递归拷贝目录（F22 迁移数据用）。返回拷贝的文件数。
/// 目标已存在同名文件则覆盖；不跟随符号链接。
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<u64> {
    let mut n = 0u64;
    if !src.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("源目录不存在：{}", src.display()),
        ));
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            n += copy_dir_recursive(&entry.path(), &target)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), &target)?;
            n += 1;
        }
    }
    Ok(n)
}

/// 去掉 Windows `canonicalize` 产生的 verbatim 前缀，得到常规显示/拼接路径：
/// `\\?\D:\dir` → `D:\dir`；`\\?\UNC\server\share` → `\\server\share`。
/// 无前缀（含 Unix 路径）或非 UTF-8（极罕见）时原样返回。
/// 注：Unix 路径不可能以 `\\?\` 开头，故无需 cfg 区分平台，便于单测。
fn strip_verbatim(path: &Path) -> PathBuf {
    if let Some(s) = path.as_os_str().to_str() {
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

static PATHS: OnceLock<AppPaths> = OnceLock::new();

/// 应用启动最早期（建窗前）调用：建目录 + 把 WebView2 缓存关进 data/webview。
pub fn init() -> io::Result<&'static AppPaths> {
    if let Some(p) = PATHS.get() {
        return Ok(p);
    }
    // 先建目录，canonicalize 去掉 ".." 得到规范绝对路径（WebView2 对路径较敏感），
    // 再剥掉 Windows 的 `\\?\` verbatim 前缀（避免扩散到 db/notes 路径与 UI 显示）。
    let p = materialize(&data_dir())?;
    let canonical = strip_verbatim(&p.data_dir.canonicalize()?);
    let webview = canonical.join("webview");

    // 仅进程内生效，随进程消失；不写注册表/不调 setx。
    // 必须早于任何 WebView2 环境初始化（故放在 Builder::run 之前）。
    // 注：edition 2021 下 set_var 为安全调用。
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview);

    let p = AppPaths {
        notes_dir: canonical.join("notes"),
        backups_dir: canonical.join("backups"),
        webview_dir: webview,
        db_path: canonical.join("todolist.db"),
        data_dir: canonical,
    };
    Ok(PATHS.get_or_init(|| p))
}

/// 取已初始化的全局路径（未初始化则 panic——只应在 init() 之后使用）。
pub fn app_paths() -> &'static AppPaths {
    PATHS.get().expect("paths::init() 尚未调用")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_under_data_root() {
        let root = PathBuf::from(r"C:\app\data");
        let p = layout(&root);
        assert_eq!(p.data_dir, root);
        assert_eq!(p.notes_dir, root.join("notes"));
        assert_eq!(p.backups_dir, root.join("backups"));
        assert_eq!(p.webview_dir, root.join("webview"));
        assert_eq!(p.db_path, root.join("todolist.db"));
    }

    #[test]
    fn materialize_creates_all_dirs() {
        // 不引 tempfile 依赖：用进程 id 造唯一临时根，测完清理。
        let base = std::env::temp_dir().join(format!(
            "todolist-paths-test-{}-{}",
            std::process::id(),
            std::line!()
        ));
        let _ = fs::remove_dir_all(&base);
        let data = base.join("data");

        let p = materialize(&data).expect("create dirs");
        assert!(p.data_dir.is_dir());
        assert!(p.notes_dir.is_dir());
        assert!(p.backups_dir.is_dir());
        assert!(p.webview_dir.is_dir());
        assert_eq!(p.db_path, data.join("todolist.db"));

        // 幂等：第二次不报错（已存在）。
        materialize(&data).expect("second call idempotent");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn strip_verbatim_prefixes() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\D:\a\b")),
            PathBuf::from(r"D:\a\b")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\srv\share\d")),
            PathBuf::from(r"\\srv\share\d")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"D:\normal")),
            PathBuf::from(r"D:\normal")
        );
        assert_eq!(
            strip_verbatim(Path::new("/unix/path")),
            PathBuf::from("/unix/path")
        );
    }

    #[test]
    fn config_path_sibling_of_default_data() {
        // 默认（无自定义 dataDir）时：root/data 与 root/app-config.json 同根。
        // 用纯函数而非 data_dir()，避免本机真的存在 app-config.json 时影响断言。
        let root = PathBuf::from("C:\\app");
        let d = resolve_data_root(&root, &AppConfig::default());
        let cfg = root.join("app-config.json");
        assert_eq!(d, root.join("data"));
        assert_eq!(d.parent(), cfg.parent());
    }

    #[test]
    fn resolve_custom_data_dir() {
        let root = PathBuf::from("C:\\app");
        // 1) 未配置 → 绿色默认
        assert_eq!(
            resolve_data_root(&root, &AppConfig::default()),
            root.join("data")
        );
        // 2) 空串/空白 → 仍用默认
        let blank = AppConfig { data_dir: Some("  ".into()), ..Default::default() };
        assert_eq!(resolve_data_root(&root, &blank), root.join("data"));
        // 3) 自定义且不以 data 结尾 → 自动补一级 data
        let custom = AppConfig { data_dir: Some("D:\\Sync\\MyTasks".into()), ..Default::default() };
        assert_eq!(
            resolve_data_root(&root, &custom),
            PathBuf::from("D:\\Sync\\MyTasks\\data")
        );
        // 4) 已以 data 结尾 → 不再补
        let already = AppConfig { data_dir: Some("D:\\Sync\\MyTasks\\data".into()), ..Default::default() };
        assert_eq!(
            resolve_data_root(&root, &already),
            PathBuf::from("D:\\Sync\\MyTasks\\data")
        );
    }

    #[test]
    fn app_config_json_roundtrip_shape() {
        // 形状固定：{"dataDir": "..."} camelCase；缺省不写该键，方便手改。
        let cfg = AppConfig { data_dir: Some("D:\\x\\data".into()), ..Default::default() };
        let s = serde_json::to_string(&cfg).unwrap();
        assert_eq!(s, r#"{"dataDir":"D:\\x\\data"}"#);
        let back: AppConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.data_dir, cfg.data_dir);
        assert_eq!(serde_json::to_string(&AppConfig::default()).unwrap(), "{}");
    }

    #[test]
    fn copy_dir_recursive_copies_tree() {
        let base = std::env::temp_dir().join(format!("todolist-copy-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let src = base.join("src");
        let dst = base.join("dst");
        fs::create_dir_all(src.join("notes")).unwrap();
        fs::write(src.join("todolist.db"), b"db").unwrap();
        fs::write(src.join("notes").join("a.md"), b"note").unwrap();

        let n = copy_dir_recursive(&src, &dst).unwrap();
        assert_eq!(n, 2, "两个文件都应拷贝");
        assert_eq!(fs::read(dst.join("todolist.db")).unwrap(), b"db");
        assert_eq!(fs::read(dst.join("notes").join("a.md")).unwrap(), b"note");

        // 源不存在 → 报错而不是静默成功
        assert!(copy_dir_recursive(&base.join("ghost"), &dst).is_err());

        let _ = fs::remove_dir_all(&base);
    }
}
