// Svelte 5 runes 全局状态单例（SPEC §6.2 stores）。
// 文件名 .svelte.ts 才能在模块顶层使用 runes。
// T0.4 仅骨架；todayTodos/memos/... 在 M1/M2 各任务填充。

export type ViewId =
  | "dashboard"
  | "today"
  | "inbox"
  | "all"
  | "work"
  | "life"
  | "projects"
  | "people"
  | "goals"
  | "memos"
  | "notes"
  | "overview"
  | "settings";

export interface NavItem {
  id: ViewId;
  label: string;
  /** 后续替换为图标组件；T0.4 先用文字符号占位 */
  icon: string;
  /** 侧栏分组小标题：区分待办/组织/沉淀等不同类实体（2026-09-22 验收反馈） */
  group: string;
}

// 分组参考旧版 Electron（视图名带类型词）与三级沉淀流水线（Todo→Memo→Note）：
// 概览=统计视图；待办=行动类；组织=项目/人员/目标；沉淀=备忘/笔记；系统=设置。
export const NAV_ITEMS: NavItem[] = [
  { id: "dashboard", label: "仪表板", icon: "▦", group: "概览" },
  { id: "overview", label: "总览", icon: "▦", group: "概览" },
  { id: "today", label: "今天", icon: "☼", group: "待办" },
  { id: "inbox", label: "收集箱", icon: "⌄", group: "待办" },
  { id: "all", label: "全部", icon: "≡", group: "待办" },
  { id: "work", label: "工作", icon: "💼", group: "待办" },
  { id: "life", label: "生活", icon: "☘", group: "待办" },
  { id: "projects", label: "项目", icon: "❏", group: "组织" },
  { id: "people", label: "人员", icon: "👤", group: "组织" },
  { id: "goals", label: "目标", icon: "◎", group: "组织" },
  { id: "memos", label: "备忘", icon: "✦", group: "沉淀" },
  { id: "notes", label: "笔记", icon: "▤", group: "沉淀" },
  { id: "settings", label: "设置", icon: "⚙", group: "系统" },
];

export type Theme = "light" | "dark" | "system";

interface UiState {
  /** 当前左导航选中视图（默认"今天"，SPEC §8.2 行动优先） */
  view: ViewId;
  theme: Theme;
  /** 标签筛选态：非空时中栏显示该标签下的待办（优先级高于 view） */
  activeTag: string | null;
  /** 右栏详情面板当前展示的待办 id；null = 空状态 */
  activeTodoId: string | null;
  /** T3.6：从月总览定位的日期（今日视图用其替代 today 加载） */
  pendingDate: number | null;
  /** 仪表板「逾期」卡跳入今日视图时，改为展示逾期未完成清单（而非今日） */
  overdueMode: boolean;
  /** 快速添加（今天/收集箱/侧栏）时新待办的分类；记住上次选择并持久化 */
  quickAddCategory: "work" | "life";
}

interface DataState {
  // 骨架，M1/M2 填充
  todos: unknown[];
  memos: unknown[];
  notes: unknown[];
  tags: unknown[];
  projects: unknown[];
  people: unknown[];
  goals: unknown[];
}

// 模块级单例，import 即共享同一份响应式状态（用纯对象，勿用 class 实例）。
export const ui = $state<UiState>({
  view: "today",
  theme: "light",
  activeTag: null,
  activeTodoId: null,
  pendingDate: null,
  overdueMode: false,
  quickAddCategory: "life",
});
export const data = $state<DataState>({
  todos: [],
  memos: [],
  notes: [],
  tags: [],
  projects: [],
  people: [],
  goals: [],
});
