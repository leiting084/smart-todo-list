//! 后端服务层：调度、导入导出、同步、AI 等非简单 CRUD 的逻辑（SPEC §6.2）。
//! P1-P3 任务填充，M0 仅占位。

pub mod ai;
pub mod exporter;
pub mod importer;
pub mod pomodoro;
pub mod reminder;
pub mod repeat;
pub mod search;
pub mod tray;
pub mod webdav;
