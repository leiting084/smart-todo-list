//! IPC 命令层：每个动作一个显式领域命令（SPEC §6.2 IPC 红线）。
//!
//! 禁止暴露通用 SQL 执行口；所有入参用 serde 强类型 struct。
//! 各模块在 M1/M2 任务中填充（TASKS T1.1 起）。

pub mod ai;
pub mod dashboard;
pub mod goal;
pub mod link;
pub mod memo;
pub mod note;
pub mod person;
pub mod project;
pub mod settings;
pub mod system;
pub mod tag;
pub mod todo;
pub mod window;
