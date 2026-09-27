// Todo 列表状态与动作（Svelte 5 runes，纯对象 $state——勿用 class，见 M0 changelog 踩坑）。
// 各视图用各自 filter 调 loadTodos；同一时刻中栏只显示一个视图。
import { api, type Todo, type TodoFilter, type UpdateTodo } from "./api";

interface TodosState {
  items: Todo[];
  loading: boolean;
  error: string;
  /** 后端认定的今天 YYYYMMDD（唯一真相源，避免前端各算各的跨天不一致） */
  today: number;
}

export const todosState = $state<TodosState>({
  items: [],
  loading: false,
  error: "",
  today: 0,
});

/** 拉取后端今天并写入 state；返回 YYYYMMDD */
export async function loadToday(): Promise<number> {
  todosState.today = await api.todayDate();
  return todosState.today;
}

/** 仅前端本地日期（用于输入框默认值等非权威场景） */
export function localTodayYmd(): number {
  const d = new Date();
  const y = d.getFullYear();
  const m = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return Number(`${y}${m}${day}`);
}

export async function loadTodos(filter: TodoFilter) {
  todosState.loading = true;
  todosState.error = "";
  try {
    todosState.items = await api.todoList(filter);
  } catch (e) {
    todosState.error = String(e);
  } finally {
    todosState.loading = false;
  }
}

function patchItem(updated: Todo) {
  todosState.items = todosState.items.map((x) =>
    x.id === updated.id ? updated : x,
  );
}

/** 有日期=进当天视图；date=null=收集箱；category 缺省时后端默认 life */
export async function addTodo(
  title: string,
  date: number | null,
  category?: "work" | "life",
) {
  const t = title.trim();
  if (!t) return;
  const todo = await api.todoCreate({ title: t, date, category });
  todosState.items = [...todosState.items, todo];
}

export async function toggleComplete(todo: Todo): Promise<Todo> {
  const updated = await api.todoSetCompleted(todo.id, !todo.completed);
  // 完成沉底：更新后重排到正确分区
  patchItem(updated);
  reorderLocal();
  return updated;
}

/** 本地按后端排序规则重排（未完成在前、完成沉底，组内 sort_order/created） */
function reorderLocal() {
  todosState.items = [...todosState.items].sort((a, b) => {
    if (a.completed !== b.completed) return a.completed ? 1 : -1;
    if (a.sortOrder !== b.sortOrder) return a.sortOrder - b.sortOrder;
    if (a.createdAt !== b.createdAt) return a.createdAt - b.createdAt;
    return a.id.localeCompare(b.id);
  });
}

export async function updateTodo(id: string, patch: UpdateTodo): Promise<Todo> {
  const updated = await api.todoUpdate(id, patch);
  patchItem(updated);
  return updated;
}

export async function removeTodo(id: string) {
  await api.todoDelete(id);
  todosState.items = todosState.items.filter((x) => x.id !== id);
}

/** 收集箱项安排到指定日期；安排后从收集箱列表移除 */
export async function scheduleTodo(id: string, date: number | null) {
  await api.todoUpdate(id, { date });
  todosState.items = todosState.items.filter((x) => x.id !== id);
}

/** 拖拽排序后持久化（同分区 id 顺序），成功后本地采用新 sortOrder */
export async function persistOrder(orderedIds: string[]) {
  await api.todoReorder(orderedIds);
  // 用返回的列表刷新（拿到新 sortOrder）；调用方通常随后 reload，这里先本地标记
  orderedIds.forEach((id, i) => {
    const t = todosState.items.find((x) => x.id === id);
    if (t) t.sortOrder = i;
  });
  reorderLocal();
}
