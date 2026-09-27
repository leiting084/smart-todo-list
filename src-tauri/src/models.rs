//! 领域模型与 IPC 入参 DTO（SPEC §5 todos 表）。
//! 对外字段统一 camelCase；DB 列 snake_case，由 SQL 显式映射。

use serde::{Deserialize, Serialize};

/// 项目（projects 表，F30 继承旧版）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub color: Option<String>,
    pub sort_order: i64,
    pub archived: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateProject {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub sort_order: Option<i64>,
}

/// 项目统计（项目详情用）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStats {
    pub todo_count: i64,
    pub completed_count: i64,
    /// 0.0–1.0；无待办为 0
    pub completion_rate: f64,
}

/// 人员（people 表，F31 继承旧版：本地责任人/相关人清单，非账号体系）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
    /// 职责（0004 起；旧版导入原拼在 note，现落独立列）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role: Option<String>,
    /// 邮箱（0004 起）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub email: Option<String>,
    pub sort_order: i64,
    pub archived: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CreatePerson {
    pub name: String,
    pub note: Option<String>,
    pub role: Option<String>,
    pub email: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdatePerson {
    pub name: Option<String>,
    pub note: Option<Option<String>>,
    pub role: Option<Option<String>>,
    pub email: Option<Option<String>>,
    pub sort_order: Option<i64>,
}

/// 一次待办的分配状态（详情/列表展示与协作勾选用）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Assignee {
    pub person_id: String,
    pub name: String,
    /// 该人是否已勾选完成
    pub completed: bool,
}

/// 长期目标（goals 表，F32 继承旧版）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    /// active / done / archived
    pub status: String,
    /// 手动进度 0-100（0005）。SPEC F32/§5 规定目标进度=关联待办完成率，
    /// 故该字段仅作为"无任何关联待办时"的回退显示；有关联待办时以 link_progress 为准。
    pub progress: i64,
    /// 目标日期（毫秒时间戳；旧版 targetDate，可空）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target_date: Option<i64>,
    /// 关联待办完成率 0.0–1.0（非 goals 列，查询时按 goal_links 计算；F32 主进度）
    #[serde(default)]
    pub link_progress: f64,
    /// 关联待办总数（非 goals 列，查询时计算）
    #[serde(default)]
    pub todo_count: i64,
    /// 关联待办已完成数（非 goals 列，查询时计算）
    #[serde(default)]
    pub done_count: i64,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateGoal {
    pub title: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub sort_order: Option<i64>,
    pub progress: Option<i64>,
    pub target_date: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateGoal {
    pub title: Option<String>,
    pub category: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub status: Option<String>,
    pub sort_order: Option<i64>,
    pub progress: Option<i64>,
    pub target_date: Option<Option<i64>>,
}

/// 目标的一条关联（goal_links）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoalLink {
    pub item_type: ItemType,
    pub item_id: String,
    /// 关联实体的标题；实体已删除时为 None（悬挂，UI 显示失效态）
    pub title: Option<String>,
    /// todo 关联时该待办是否完成
    pub completed: bool,
}

/// 目标统计：进度 = 关联待办完成率；备忘/笔记计入关联数展示（SPEC F32）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GoalStats {
    pub todo_count: i64,
    pub completed_count: i64,
    /// 0.0–1.0；无关联待办为 0
    pub progress: f64,
    pub memo_count: i64,
    pub note_count: i64,
}

/// goal_set_links 的入参项。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalLinkInput {
    pub item_type: ItemType,
    pub item_id: String,
}

/// 备忘（memos 表，L2 记录层，F3）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Memo {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
    /// 标签名（list 时填充）
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateMemo {
    pub title: String,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateMemo {
    pub title: Option<String>,
    pub content: Option<Option<String>>,
}

/// 一条双向链接（links 表）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub id: String,
    pub from_type: ItemType,
    pub from_id: String,
    pub to_type: ItemType,
    pub to_id: String,
    /// converted_to / derived_from / backlog / reference
    pub relation: String,
    pub created_at: i64,
}

/// SPEC §5 七种待办颜色
pub const COLORS: [&str; 7] = [
    "red", "orange", "yellow", "green", "blue", "purple", "gray",
];
pub const CATEGORIES: [&str; 2] = ["work", "life"];
pub const PRIORITIES: [&str; 3] = ["high", "medium", "low"];

/// todos 表一行（也作为命令返回给前端的视图模型）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content: Option<String>,
    pub completed: bool,
    /// YYYYMMDD；None = 收集箱
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub date: Option<i64>,
    pub sort_order: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub color: Option<String>,
    pub category: String,
    pub priority: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub project_id: Option<String>,
    /// HH:MM
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_id: Option<String>,
    pub postpone_auto: bool,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub completed_at: Option<i64>,
    /// 标签名列表（非 todos 列，list 查询后批量填充；单条 create/update 可能为空）
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 标签（tags 表）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tag {
    pub id: String,
    pub name: String,
}

/// 标签 + 其下实体数量（侧栏标签树用；空标签 count=0 由前端隐藏）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub id: String,
    pub name: String,
    pub total: i64,
    pub todos: i64,
    pub memos: i64,
    pub notes: i64,
}

/// 一个三级实体的类型（item_tags / links 共用）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    Todo,
    Memo,
    Note,
}

impl ItemType {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemType::Todo => "todo",
            ItemType::Memo => "memo",
            ItemType::Note => "note",
        }
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "todo" => Ok(ItemType::Todo),
            "memo" => Ok(ItemType::Memo),
            "note" => Ok(ItemType::Note),
            other => Err(format!("非法 item_type：{other}")),
        }
    }
}

/// todo_create 入参。id / 时间戳 / completed 由后端写。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTodo {
    pub title: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub date: Option<i64>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

/// todo_update 入参（部分更新）。
/// - title/category/priority：普通 Option，给值即更新（不允许清空成空串）；
/// - 可空字段用 `Option<Option<T>>`：缺失=不改，显式 null=清空。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateTodo {
    pub title: Option<String>,
    pub content: Option<Option<String>>,
    pub date: Option<Option<i64>>,
    pub color: Option<Option<String>>,
    pub category: Option<String>,
    pub priority: Option<String>,
    pub project_id: Option<Option<String>>,
    pub time: Option<Option<String>>,
    pub parent_id: Option<Option<String>>,
    pub sort_order: Option<i64>,
}

/// todo_batch_update 入参（多选批量操作，部分更新）：
/// - 字段缺失=不改该字段；可空字段用 `Option<Option<T>>`：缺失=不改，显式 null=清空。
/// - completed 语义对齐月总览批量完成（todo_assignees 先行、不触发转备忘）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BatchTodoPatch {
    pub completed: Option<bool>,
    /// None=不改；Some(None)=清空进收集箱；Some(Some(d))=改为该日
    pub date: Option<Option<i64>>,
    pub priority: Option<String>,
    pub color: Option<Option<String>>,
    pub category: Option<String>,
    pub project_id: Option<Option<String>>,
}

/// todo_list 过滤条件（全部可选，AND 组合）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TodoFilter {
    pub date: Option<i64>,
    /// true=仅收集箱（date IS NULL）
    pub inbox: Option<bool>,
    pub completed: Option<bool>,
    pub category: Option<String>,
    pub priority: Option<String>,
    pub project_id: Option<String>,
    /// 仅返回分配给该 person 的待办
    pub assignee: Option<String>,
    /// 关键词：标题/描述/标签名模糊
    pub q: Option<String>,
    /// 精确按标签名筛选（侧栏标签树）
    pub tag: Option<String>,
    /// 排序方式：None/其它=默认（未完成置顶+sort_order+创建时间）；"priority"=按优先级 高→中→低
    pub sort: Option<String>,
}
