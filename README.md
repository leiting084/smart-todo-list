# 智能待办清单

一个完全离线的 Windows 桌面待办应用：**绿色便携版**，解压即用，数据就放在软件自己的目录里，不写注册表、不往 AppData 里塞东西。

技术栈：Rust + Tauri 2 + SQLite（rusqlite，WAL） + Svelte 5 / SvelteKit（static adapter）。

---

## 为什么又做一个

需求很简单，但市面上的待办软件大多做不到：

1. **数据必须在我自己的文件夹里**——我想把它丢进 U 盘或网盘同步目录就丢进去；
2. **待办不是终点**——做完的事要能沉淀成备忘，再整理成笔记，笔记里还能反推出新的待办；
3. **离线优先**——不强制注册、不强制同步、不上传任何东西。

于是有了这个：待办 → 备忘 → 笔记的三级沉淀闭环，加上项目 / 人员协作 / 长期目标三张旧版就有的骨架表。

---

## 功能

### 三级沉淀（核心差异点）

| 层 | 做什么 |
|---|---|
| 待办 | 今日视图 + 收集箱、子待办（3 层嵌套）、重复（每日/每周/每月）、到期提醒、拖拽排序与跨日期拖拽改期 |
| 备忘 | 勾选完成即可一键转备忘（单事务 + 成对链接 + 标签复制） |
| 笔记 | Markdown 文件存储（笔记正文是真实的 `.md` 文件，可用别的编辑器改）、备忘多选整理为笔记、笔记内 `[[todo:ID]]` wiki-link 生成反链 |

### 日常能力

- 项目、人员（多人协作完成：全员勾选才算整体完成）、长期目标（进度 = 关联待办完成率）
- 标签（三类实体通用，重命名全局生效）
- 全文搜索（SQLite FTS5 trigram，中文子串可命中）
- 月总览日历（含法定节假日与调休标记）、仪表板（纯 SQL 聚合）
- 迷你浮窗（置顶 / 置底 / 透明度 / 贴边隐藏）、系统托盘、全局快捷键、开机自启（不弹主窗）
- 深色模式（含跟随系统）、窗口位置记忆
- 导出 Markdown / CSV（带 UTF-8 BOM）/ HTML
- 导入：旧版 JSON、剪贴板多行文本
- 自定义数据目录（可指向网盘同步文件夹，作为多机同步的替代方案）
- AI 周报（**可选**：自己填 endpoint 与 key，不填就完全不外联）

---

## 数据存在哪

这是本项目最在意的一件事，规则写死在代码里：

- **release 版**：`<exe 所在目录>/data/`，`app-config.json` 也在 exe 同级
- **dev 版**：项目根 `data/`
- 运行时缓存（WebView2）也被强制指到 `data/webview/`
- 所有路径决策集中在 `src-tauri/src/paths.rs` 一个文件里，别处不许碰环境变量或注册表

表现为：把整个文件夹复制到另一台机器，数据、链接、笔记跟着一起走。

---

## 构建

前置：Node 22+、pnpm 10+、Rust 1.85+、Windows（依赖 WebView2 Runtime）。

```bash
pnpm install
pnpm tauri dev          # 开发模式
pnpm build:green        # 产出绿色便携目录 + zip
```

质量门：

```bash
cargo test --release                                   # 单元测试
cargo clippy --release --all-targets -- -D warnings    # 零告警
pnpm check                                             # svelte-check
```

> 本项目**只发布绿色便携目录**，不构建 NSIS / MSI 安装包：`tauri.conf.json` 里
> `bundle.active = false`，打包入口统一是 `pnpm build:green`（`scripts/build-green.mjs`）。

---

## 目录结构

```
src/                     前端（Svelte 5 + SvelteKit static）
  lib/api.ts             invoke 的唯一出口
  lib/views/             各视图
  lib/components/        TodoList / DateStrip / LinkPanel / AiReport ...
  routes/mini/           迷你浮窗页面
src-tauri/
  src/paths.rs           数据路径的唯一真相源
  src/db.rs              连接、WAL、版本化迁移、升级前备份
  src/migrations/        0001 起，只增不改
  src/commands/          IPC 命令（每个命令用 serde struct 收参，无通用 SQL 口）
  src/services/          业务：导入/导出/搜索/重复/提醒/AI
scripts/
  build-green.mjs        绿色便携版打包入口
```

---

## 隐私

- 不联网、不埋点、不上传任何数据（AI 周报是可选功能，不填 endpoint 就没有任何外联）
- AI 的 key 只存在本地 `app-config.json`，日志里打码
- 数据库未加密：这是本地单机形态的有意取舍，物理访问场景交给 BitLocker / 系统登录凭据

---

## 许可

MIT，见 [LICENSE](./LICENSE)。
