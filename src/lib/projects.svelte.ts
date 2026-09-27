// 项目状态（Svelte 5 runes，纯对象 $state）。
import { api, type Project } from "./api";

interface ProjectsState {
  list: Project[];
  loading: boolean;
}

export const projectsState = $state<ProjectsState>({ list: [], loading: false });

export async function loadProjects(includeArchived = false) {
  projectsState.loading = true;
  try {
    projectsState.list = await api.projectList(includeArchived);
  } finally {
    projectsState.loading = false;
  }
}

export async function createProject(name: string, color?: string) {
  const p = await api.projectCreate({ name, color: color ?? null });
  projectsState.list = [...projectsState.list, p];
  return p;
}

export async function updateProject(id: string, patch: Parameters<typeof api.projectUpdate>[1]) {
  const p = await api.projectUpdate(id, patch);
  projectsState.list = projectsState.list.map((x) => (x.id === id ? p : x));
  return p;
}

/** 归档/恢复；归档后从活动列表移除（下次加载） */
export async function setProjectArchived(id: string, archived: boolean) {
  await api.projectArchive(id, archived);
  projectsState.list = projectsState.list.filter((x) =>
    archived ? x.id !== id : true,
  );
}
