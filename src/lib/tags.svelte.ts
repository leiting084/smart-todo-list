// 标签状态（Svelte 5 runes，纯对象 $state）。侧栏标签树与各视图共用。
import { api, type ItemType, type TagCount } from "./api";
import { todosState } from "./todos.svelte";

interface TagsState {
  list: TagCount[];
}

export const tagsState = $state<TagsState>({ list: [] });

export async function loadTags() {
  tagsState.list = await api.tagsList();
}

/** 整体替换某实体的标签（逗号语义由后端去重去空）；同步本地 todo 与标签树计数 */
export async function setItemTags(
  itemType: ItemType,
  itemId: string,
  names: string[],
) {
  await api.itemSetTags(itemType, itemId, names);
  if (itemType === "todo") {
    const t = todosState.items.find((x) => x.id === itemId);
    if (t) t.tags = names;
  }
  await loadTags();
}

/** 删除标签（不删实体），刷新标签树 */
export async function removeTag(id: string) {
  await api.tagDelete(id);
  await loadTags();
}
