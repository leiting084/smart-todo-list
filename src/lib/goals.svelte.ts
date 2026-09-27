// 目标状态（Svelte 5 runes，纯对象 $state）。
import { api, type Goal } from "./api";

interface GoalsState {
  list: Goal[];
}

export const goalsState = $state<GoalsState>({ list: [] });

export async function loadGoals(includeArchived = false) {
  goalsState.list = await api.goalList(includeArchived);
}

export async function createGoal(title: string, category?: string) {
  const g = await api.goalCreate({ title, category: category ?? null });
  goalsState.list = [...goalsState.list, g];
  return g;
}

/** 标记达成（status=done）或回到进行中 */
export async function setGoalStatus(id: string, status: string) {
  const g = await api.goalUpdate(id, { status });
  goalsState.list = goalsState.list.map((x) => (x.id === id ? g : x));
  return g;
}

/** 通用局部更新（手动进度 / 目标日期等，0005） */
export async function patchGoal(id: string, patch: { progress?: number; targetDate?: number | null }) {
  const g = await api.goalUpdate(id, patch);
  goalsState.list = goalsState.list.map((x) => (x.id === id ? g : x));
  return g;
}

/** 删除=归档 */
export async function archiveGoal(id: string) {
  await api.goalDelete(id);
  goalsState.list = goalsState.list.filter((x) => x.id !== id);
}
