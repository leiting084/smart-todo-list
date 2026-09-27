// 对 Tauri invoke 的全部封装——前端唯一允许 import @tauri-apps/api 的地方（SPEC §6.2）。
// 其余组件/视图只允许 import 本模块，不直接 invoke。
import { invoke } from "@tauri-apps/api/core";

// ---------- 领域模型（camelCase，对应 Rust models.rs） ----------

export interface Todo {
  id: string;
  title: string;
  content?: string | null;
  completed: boolean;
  /** YYYYMMDD；null/缺省 = 收集箱 */
  date?: number | null;
  sortOrder: number;
  color?: string | null;
  category: "work" | "life";
  priority: "high" | "medium" | "low";
  projectId?: string | null;
  time?: string | null;
  parentId?: string | null;
  postponeAuto: boolean;
  createdAt: number;
  updatedAt: number;
  completedAt?: number | null;
  /** 标签名（list 查询时由后端批量填充） */
  tags: string[];
}

export interface CreateTodo {
  title: string;
  content?: string | null;
  date?: number | null;
  color?: string | null;
  category?: "work" | "life";
  priority?: "high" | "medium" | "low";
  projectId?: string | null;
  time?: string | null;
  parentId?: string | null;
  sortOrder?: number;
}

/** 部分更新；可空字段传 null 表示清空，不传表示不改 */
export interface UpdateTodo {
  title?: string;
  content?: string | null;
  date?: number | null;
  color?: string | null;
  category?: "work" | "life";
  priority?: "high" | "medium" | "low";
  projectId?: string | null;
  time?: string | null;
  parentId?: string | null;
  sortOrder?: number;
}

export interface TodoFilter {
  date?: number | null;
  inbox?: boolean;
  completed?: boolean;
  category?: "work" | "life";
  priority?: "high" | "medium" | "low";
  projectId?: string;
  assignee?: string;
  /** 关键词：标题/描述/标签名模糊 */
  q?: string;
  /** 精确按标签名筛选 */
  tag?: string;
  /** 排序方式："priority"=按优先级 高→中→低；省略=默认（手动拖拽序） */
  sort?: "priority";
}

export interface Tag {
  id: string;
  name: string;
}

export interface TagCount {
  id: string;
  name: string;
  total: number;
  todos: number;
  memos: number;
  notes: number;
}

export type ItemType = "todo" | "memo" | "note";

// ---- 项目（T1.5） ----
export interface Project {
  id: string;
  name: string;
  description?: string | null;
  color?: string | null;
  sortOrder: number;
  archived: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface CreateProject {
  name: string;
  description?: string | null;
  color?: string | null;
  sortOrder?: number;
}

export interface UpdateProject {
  name?: string;
  description?: string | null;
  color?: string | null;
  sortOrder?: number;
}

export interface ProjectStats {
  todoCount: number;
  completedCount: number;
  completionRate: number;
}

// ---- 人员（T1.6） ----
export interface Person {
  id: string;
  name: string;
  /** 职责说明（如「前端」「设计」） */
  role?: string | null;
  email?: string | null;
  note?: string | null;
  sortOrder: number;
  archived: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface CreatePerson {
  name: string;
  role?: string | null;
  email?: string | null;
  note?: string | null;
  sortOrder?: number;
}

/** 可空字段传 null 表示清空，不传表示不改 */
export interface UpdatePerson {
  name?: string;
  role?: string | null;
  email?: string | null;
  note?: string | null;
  sortOrder?: number;
}

export interface Assignee {
  personId: string;
  name: string;
  completed: boolean;
}

// ---- 长期目标（T1.7） ----
export interface Goal {
  id: string;
  title: string;
  category?: string | null;
  description?: string | null;
  /** active / done / archived */
  status: string;
  /** 手动进度 0-100：仅当无关联待办时作为回退显示；有关联待办时以 linkProgress 为准 */
  progress: number;
  /** 目标日期（毫秒时间戳，可空） */
  targetDate?: number | null;
  /** 关联待办完成率 0.0–1.0（F32 主进度） */
  linkProgress: number;
  /** 关联待办总数 */
  todoCount: number;
  /** 关联待办已完成数 */
  doneCount: number;
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
}

export interface CreateGoal {
  title: string;
  category?: string | null;
  description?: string | null;
  sortOrder?: number;
  progress?: number;
  targetDate?: number | null;
}

export interface UpdateGoal {
  title?: string;
  category?: string | null;
  description?: string | null;
  status?: string;
  sortOrder?: number;
  progress?: number;
  /** null = 清空目标日期 */
  targetDate?: number | null;
}

export interface GoalLink {
  itemType: ItemType;
  itemId: string;
  /** 实体已删除时为 null（悬挂失效态） */
  title: string | null;
  completed: boolean;
}

export interface GoalLinkInput {
  itemType: ItemType;
  itemId: string;
}

export interface GoalStats {
  todoCount: number;
  completedCount: number;
  /** 0.0–1.0 */
  progress: number;
  memoCount: number;
  noteCount: number;
}

// ---- 仪表板（T1.8） ----
export interface ProjectSlice {
  projectId: string;
  name: string;
  color?: string | null;
  total: number;
  done: number;
}

export interface GoalSlice {
  goalId: string;
  title: string;
  todoCount: number;
  doneCount: number;
  progress: number;
}

export interface TrendPoint {
  /** YYYYMMDD */
  date: number;
  completed: number;
}

export interface DashboardStats {
  todayCount: number;
  overdueCount: number;
  weekCompleted: number;
  projectDistribution: ProjectSlice[];
  goalProgress: GoalSlice[];
  weekTrend: TrendPoint[];
}

// ---- 旧版导入（T1.9） ----
export interface ImportReport {
  tasks: number;
  projects: number;
  people: number;
  tags: number;
  goals: number;
  skipped: string[];
  notes: string[];
  /** true=因指纹相同被跳过，未写入任何数据 */
  alreadyImported: boolean;
}

// ---- 备忘（T2.1） ----
export interface Memo {
  id: string;
  title: string;
  content?: string | null;
  createdAt: number;
  updatedAt: number;
  archived: boolean;
  tags: string[];
}

export interface CreateMemo {
  title: string;
  content?: string | null;
}

export interface UpdateMemo {
  title?: string;
  content?: string | null;
}

// ---- 链接（T2.1/T2.4） ----
export interface Link {
  id: string;
  fromType: ItemType;
  fromId: string;
  toType: ItemType;
  toId: string;
  /** converted_to / derived_from / backlog / reference */
  relation: string;
  createdAt: number;
}

// ---- 笔记（T2.2） ----
export interface Note {
  id: string;
  title: string;
  /** 相对 data/ 路径，如 notes/<id>.md */
  filePath: string;
  createdAt: number;
  updatedAt: number;
  archived: boolean;
}

export interface NoteContent {
  note: Note;
  content: string;
}

// ---- 最近操作只读日志（#32） ----
export interface OpLogEntry {
  id: number;
  /** 毫秒时间戳 */
  ts: number;
  /** create/update/delete/complete/uncomplete/archive/unarchive/import */
  action: string;
  /** todo/project/memo/note/goal/person */
  entityType: string;
  /** 目标 id；批量操作为 null（后端 None 时省略该字段） */
  entityId?: string | null;
  /** 人类可读摘要 */
  summary: string;
}

export const api = {
  // ---- 系统 / 设置 ----
  ping: () => invoke<string>("ping"),
  appDataDir: () => invoke<string>("app_data_dir"),
  todayDate: () => invoke<number>("today_date"),
  /** 自定义数据目录信息（T4.3/F22） */
  getDataDirInfo: () => invoke<DataDirInfo>("get_data_dir_info"),
  /** 切换数据目录；null/空串 = 恢复绿色默认。写完配置须重启生效 */
  setDataDir: (target: string | null) =>
    invoke<DataDirInfo>("set_data_dir", { target }),
  /** 重启应用（切换到新数据目录后调用） */
  relaunchApp: () => invoke<void>("relaunch_app"),
  // ---- 浮窗设置（F12 补齐：置顶/置底 · 透明度 · 贴边隐藏）----
  miniGetSettings: () => invoke<MiniSettings>("mini_get_settings"),
  miniSetPinMode: (mode: "top" | "bottom" | "none") =>
    invoke<MiniSettings>("mini_set_pin_mode", { mode }),
  miniSetOpacity: (opacity: number) =>
    invoke<MiniSettings>("mini_set_opacity", { opacity }),
  miniSetEdgeHide: (on: boolean) =>
    invoke<MiniSettings>("mini_set_edge_hide", { on }),
  miniEdgeCollapse: () => invoke<void>("mini_edge_collapse"),
  miniEdgeExpand: () => invoke<void>("mini_edge_expand"),
  // ---- AI 周报（F26/T5.1）----
  aiGetConfig: () => invoke<AiConfigView>("ai_get_config"),
  aiSaveProviders: (providers: AiProvider[], currentProviderId: string | null) =>
    invoke<AiConfigView>("ai_save_providers", { providers, currentProviderId }),
  aiTestConnection: (providerId: string) =>
    invoke<string[]>("ai_test_connection", { providerId }),
  aiSetPrompt: (prompt: string) => invoke<AiConfigView>("ai_set_prompt", { prompt }),
  /** 立即返回，结果经 ai://chunk / ai://done / ai://error 事件推送 */
  aiGenerate: (providerId: string, userPrompt: string) =>
    invoke<void>("ai_generate", { providerId, userPrompt }),
  aiCancel: () => invoke<void>("ai_cancel"),
  // ---- 更新检查（F27/T5.2）----
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  setUpdateUrl: (url: string) => invoke<string>("set_update_url", { url }),
  settingsGet: (key: string) =>
    invoke<string | null>("settings_get", { key }),
  settingsSet: (key: string, value: string) =>
    invoke<void>("settings_set", { key, value }),

  // ---- Todo ----
  todoCreate: (input: CreateTodo) => invoke<Todo>("todo_create", { input }),
  /** 单条读取（详情面板；含标签） */
  todoGet: (id: string) => invoke<Todo | null>("todo_get", { id }),
  todoUpdate: (id: string, input: UpdateTodo) =>
    invoke<Todo>("todo_update", { id, input }),
  todoDelete: (id: string) => invoke<void>("todo_delete", { id }),
  todoSetCompleted: (id: string, completed: boolean) =>
    invoke<Todo>("todo_set_completed", { id, completed }),
  todoList: (filter?: TodoFilter) =>
    invoke<Todo[]>("todo_list", { filter: filter ?? null }),
  todoReorder: (orderedIds: string[]) =>
    invoke<void>("todo_reorder", { orderedIds }),
  /** 连带完成：父 + 全部后代（T3.1），返回完成条数 */
  todoCompleteWithChildren: (id: string) =>
    invoke<number>("todo_complete_with_children", { id }),
  /** 批量部分更新（多选模式），返回更新条数 */
  todoBatchUpdate: (ids: string[], patch: BatchTodoPatch) =>
    invoke<number>("todo_batch_update", { ids, patch }),
  /** 批量删除（二次确认由前端做），返回删除条数 */
  todoBatchDelete: (ids: string[]) =>
    invoke<number>("todo_batch_delete", { ids }),

  // ---- 标签（T1.3） ----
  tagCreate: (name: string) => invoke<Tag>("tag_create", { name }),
  tagRename: (id: string, name: string) =>
    invoke<void>("tag_rename", { id, name }),
  tagDelete: (id: string) => invoke<void>("tag_delete", { id }),
  itemSetTags: (itemType: ItemType, itemId: string, names: string[]) =>
    invoke<void>("item_set_tags", { itemType, itemId, names }),
  tagsList: () => invoke<TagCount[]>("tags_list"),
  itemsByTag: (name: string, itemType?: ItemType) =>
    invoke<string[]>("items_by_tag", { name, itemType: itemType ?? null }),

  // ---- 项目（T1.5） ----
  projectCreate: (input: CreateProject) =>
    invoke<Project>("project_create", { input }),
  projectUpdate: (id: string, input: UpdateProject) =>
    invoke<Project>("project_update", { id, input }),
  projectArchive: (id: string, archived: boolean) =>
    invoke<void>("project_archive", { id, archived }),
  /** 删除=归档 */
  projectDelete: (id: string) => invoke<void>("project_delete", { id }),
  projectList: (includeArchived = false) =>
    invoke<Project[]>("project_list", { includeArchived }),
  projectStats: (id: string) =>
    invoke<ProjectStats>("project_get_stats", { id }),

  // ---- 人员与协作完成（T1.6） ----
  personCreate: (input: CreatePerson) =>
    invoke<Person>("person_create", { input }),
  personUpdate: (id: string, input: UpdatePerson) =>
    invoke<Person>("person_update", { id, input }),
  personArchive: (id: string, archived: boolean) =>
    invoke<void>("person_archive", { id, archived }),
  personList: (includeArchived = false) =>
    invoke<Person[]>("person_list", { includeArchived }),
  todoGetAssignees: (todoId: string) =>
    invoke<Assignee[]>("todo_get_assignees", { todoId }),
  todoSetAssignees: (todoId: string, personIds: string[]) =>
    invoke<Assignee[]>("todo_set_assignees", { todoId, personIds }),
  /** 分配人自己的勾选（协作完成语义） */
  todoPersonToggle: (todoId: string, personId: string, completed: boolean) =>
    invoke<Assignee[]>("todo_person_toggle", { todoId, personId, completed }),

  // ---- 长期目标（T1.7） ----
  goalCreate: (input: CreateGoal) => invoke<Goal>("goal_create", { input }),
  goalUpdate: (id: string, input: UpdateGoal) =>
    invoke<Goal>("goal_update", { id, input }),
  /** 删除=归档 */
  goalDelete: (id: string) => invoke<void>("goal_delete", { id }),
  goalList: (includeArchived = false) =>
    invoke<Goal[]>("goal_list", { includeArchived }),
  goalSetLinks: (goalId: string, items: GoalLinkInput[]) =>
    invoke<GoalLink[]>("goal_set_links", { goalId, items }),
  goalGetLinks: (goalId: string) =>
    invoke<GoalLink[]>("goal_get_links", { goalId }),
  goalGetStats: (goalId: string) =>
    invoke<GoalStats>("goal_get_stats", { goalId }),

  // ---- 仪表板（T1.8） ----
  dashboardGet: () => invoke<DashboardStats>("dashboard_get"),

  // ---- 旧版导入（T1.9） ----
  /** content = 前端 FileReader 读入的 JSON 文本 */
  importLegacyJson: (content: string) =>
    invoke<ImportReport>("import_legacy_json", { content }),

  // ---- 链接（T2.1/T2.4） ----
  linksForEntity: (itemType: ItemType, itemId: string) =>
    invoke<Link[]>("links_for_entity", { itemType, itemId }),

  // ---- 备忘（T2.1） ----
  memoCreate: (input: CreateMemo) => invoke<Memo>("memo_create", { input }),
  memoUpdate: (id: string, input: UpdateMemo) =>
    invoke<Memo>("memo_update", { id, input }),
  memoDelete: (id: string) => invoke<void>("memo_delete", { id }),
  memoList: (includeArchived = false) =>
    invoke<Memo[]>("memo_list", { includeArchived }),
  /** 完成转备忘（F3 单事务：memo + 成对 link + 标签复制） */
  memoCreateFromTodo: (todoId: string) =>
    invoke<Memo>("memo_create_from_todo_cmd", { todoId }),
  /** 备忘整理为笔记（F4）：noteId 空=新建（title），否则追加 */
  memoToNote: (memoIds: string[], noteId: string | null, title: string) =>
    invoke<Note>("memo_to_note_cmd", { memoIds, noteId, title }),

  // ---- 笔记（T2.2） ----
  noteCreate: (input: { title: string; content?: string | null }) =>
    invoke<Note>("note_create", { input }),
  noteRename: (id: string, title: string) =>
    invoke<Note>("note_rename", { id, title }),
  /** 删除：md 移入 .trash，行归档 */
  noteDelete: (id: string) => invoke<void>("note_delete", { id }),
  noteList: (includeArchived = false) =>
    invoke<Note[]>("note_list", { includeArchived }),
  noteGet: (id: string) => invoke<Note>("note_get", { id }),
  /** 读正文；外部修改过（mtime）以文件为准并刷新 updated_at */
  noteGetContent: (id: string) => invoke<NoteContent>("note_get_content", { id }),
  noteSaveContent: (id: string, content: string) =>
    invoke<Note>("note_save_content", { id, content }),
  /** 笔记生成待办（F6/T2.4）：backlog(note→todo)+reference(todo→note) 成对链接 */
  noteCreateTodo: (noteId: string, text: string, date?: number | null) =>
    invoke<Todo>("note_create_todo_cmd", { noteId, text, date: date ?? null }),


// ---- 重复待办（T3.5） ----
  /** 生成未来 7 天+补过去 1 天实例，返回新增数 */
  repeatGenerate: () => invoke<number>("repeat_generate"),
  /** 自动顺延：逾期未完成的非重复待办 → 今天，返回条数 */
  repeatPostpone: () => invoke<number>("repeat_postpone"),


// ---- 月总览（T3.6） ----
  overviewMonth: (year: number, month: number) =>
    invoke<MonthStats>("overview_month", { year, month }),
  /** 批量完成某日全部未完成（单事务），返回条数 */
  overviewBatchComplete: (date: number) =>
    invoke<number>("overview_batch_complete", { date }),
  /** 批量取消完成某日全部已完成（单事务），返回条数 */
  overviewBatchUncomplete: (date: number) =>
    invoke<number>("overview_batch_uncomplete", { date }),


  /** 剪贴板批量创建（T3.9）：分批（200/事务）入库，返回建成数/失败明细 */
  todosBulkCreate: (items: CreateTodo[]) =>
    invoke<[number, string[]]>("todos_bulk_create", { items }),

  // ---- 导出（T4.1） ----
  exportMarkdown: (path: string, filter?: ExportFilter | null) =>
    invoke<[string, number]>("export_todos_markdown", { path, filter: filter ?? null }),
  exportCsv: (path: string, filter?: ExportFilter | null) =>
    invoke<[string, number]>("export_todos_csv", { path, filter: filter ?? null }),
  exportHtml: (path: string, filter?: ExportFilter | null) =>
    invoke<[string, number]>("export_todos_html", { path, filter: filter ?? null }),

  // ---- 搜索（T4.5） ----
  searchAll: (q: string) => invoke<SearchResult>("search_all", { q }),

  // ---- 最近操作只读日志（#32） ----
  /** 读取最近操作记录（新→旧），后端裁剪到最多 500 条 */
  oplogList: (limit = 100) => invoke<OpLogEntry[]>("oplog_list", { limit }),
};

/** settings 键名集中处，避免散落字符串 */
export const SETTING_KEYS = {
  theme: "theme",
  defaultCategory: "default_category",
} as const;

// ---- 月总览（T3.6）类型 ----
export interface DayStat {
  date: number;
  total: number;
  done: number;
  colors: string[];
}

export interface MonthStats {
  days: DayStat[];
  monthTotal: number;
  monthDone: number;
  inboxCount: number;
}

// ---- 剪贴板批量导入（T3.9）类型 ----
export interface BulkResult {
  created: number;
  failed: string[];
}

// ---- 导出（T4.1）类型 ----
export interface ExportFilter {
  dateFrom?: number | null;
  dateTo?: number | null;
  projectId?: string | null;
  category?: "work" | "life" | null;
  includeDone?: boolean | null;
}

// ---- 搜索（T4.5）类型 ----
export interface SearchResult {
  todos: Todo[];
  memos: Memo[];
  notes: Note[];
}

// ---- 浮窗设置（F12 补齐）类型 ----
export interface MiniSettings {
  /** "top" 置顶 | "bottom" 置底（钉桌面） | "none" 普通窗口 */
  pin_mode: string;
  /** 0.35–1.0；1.0 = 完全不透明 */
  opacity: number;
  /** 鼠标移出后缩成边条贴在最近的屏幕边缘，移入展开 */
  edge_hide: boolean;
}

// ---- AI 周报（F26/T5.1）类型：字段与 Rust 侧一致（snake_case）----
export interface AiProvider {
  id: string;
  name: string;
  base_url: string;
  api_key: string;
  model: string;
}
export interface ProviderView {
  id: string;
  name: string;
  base_url: string;
  model: string;
  has_key: boolean;
  key_hint: string;
}
export interface AiConfigView {
  providers: ProviderView[];
  current_provider_id: string | null;
  system_prompt: string;
}

// ---- 更新检查（F27/T5.2）类型 ----
export interface UpdateInfo {
  current: string;
  latest: string;
  hasUpdate: boolean;
  url: string;
}

// ---- 自定义数据目录（T4.3/F22）类型 ----
export interface DataDirInfo {
  /** 当前生效的数据根目录 */
  dataDir: string;
  /** 应用安装根目录（app-config.json 所在） */
  appRoot: string;
  configPath: string;
  /** true = 自定义目录；false = 绿色默认（app 同级 data/） */
  isCustom: boolean;
}

// ---- 批量部分更新（多选模式）类型 ----
export interface BatchTodoPatch {
  completed?: boolean;
  /** 不传=不改；null=清空进收集箱；数字=YYYYMMDD */
  date?: number | null;
  priority?: "high" | "medium" | "low";
  color?: string | null; // red/orange/yellow/green/blue/purple/gray
  category?: "work" | "life";
  projectId?: string | null;
}
