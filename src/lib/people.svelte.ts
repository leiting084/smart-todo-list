// 人员状态（Svelte 5 runes，纯对象 $state）。
import { api, type Person } from "./api";

interface PeopleState {
  list: Person[];
}

export const peopleState = $state<PeopleState>({ list: [] });

export async function loadPeople(includeArchived = false) {
  peopleState.list = await api.personList(includeArchived);
}

export async function createPerson(
  name: string,
  role?: string | null,
  email?: string | null,
  note?: string | null,
) {
  const p = await api.personCreate({
    name,
    role: role ?? null,
    email: email ?? null,
    note: note ?? null,
  });
  peopleState.list = [...peopleState.list, p];
  return p;
}

export async function setPersonArchived(id: string, archived: boolean) {
  await api.personArchive(id, archived);
  peopleState.list = peopleState.list.filter((x) => (archived ? x.id !== id : true));
}
