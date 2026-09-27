// 备忘状态（Svelte 5 runes，纯对象 $state）。
import { api, type Memo } from "./api";

interface MemosState {
  list: Memo[];
}

export const memosState = $state<MemosState>({ list: [] });

export async function loadMemos(includeArchived = false) {
  memosState.list = await api.memoList(includeArchived);
}

/** F3：完成转备忘（后端单事务）并刷新列表 */
export async function convertTodoToMemo(todoId: string): Promise<Memo> {
  const memo = await api.memoCreateFromTodo(todoId);
  memosState.list = [memo, ...memosState.list];
  return memo;
}

export async function updateMemo(id: string, patch: Parameters<typeof api.memoUpdate>[1]) {
  const m = await api.memoUpdate(id, patch);
  memosState.list = memosState.list.map((x) => (x.id === id ? m : x));
  return m;
}

/** 删除=归档 */
export async function archiveMemo(id: string) {
  await api.memoDelete(id);
  memosState.list = memosState.list.filter((x) => x.id !== id);
}
