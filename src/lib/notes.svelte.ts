// 笔记状态（Svelte 5 runes，纯对象 $state）。
import { api, type Note } from "./api";

interface NotesState {
  list: Note[];
}

export const notesState = $state<NotesState>({ list: [] });

export async function loadNotes(includeArchived = false) {
  notesState.list = await api.noteList(includeArchived);
}

export async function createNote(title: string, content?: string): Promise<Note> {
  const n = await api.noteCreate({ title, content: content ?? null });
  notesState.list = [n, ...notesState.list];
  return n;
}

export async function renameNote(id: string, title: string) {
  const n = await api.noteRename(id, title);
  notesState.list = notesState.list.map((x) => (x.id === id ? n : x));
  return n;
}

/** 删除：文件进 .trash，行归档 */
export async function deleteNote(id: string) {
  await api.noteDelete(id);
  notesState.list = notesState.list.filter((x) => x.id !== id);
}
