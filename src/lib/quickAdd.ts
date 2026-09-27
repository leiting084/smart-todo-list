// 快速添加（今天/收集箱/侧栏）的新待办分类：跨视图共享 + 记住上次选择。
// 值存在 settings 表（与主题同款 KV），启动时由 initQuickAddCategory 恢复到 ui。
import { api, SETTING_KEYS } from "./api";
import { ui } from "./stores.svelte";

/** 启动：从 settings 恢复上次选择的分类；未设置/非法则保持默认 life。 */
export async function initQuickAddCategory(): Promise<void> {
  try {
    const saved = await api.settingsGet(SETTING_KEYS.defaultCategory);
    if (saved === "work" || saved === "life") ui.quickAddCategory = saved;
  } catch {
    /* 浏览器环境无 IPC，忽略 */
  }
}

/** 切换分类：即时更新共享状态并持久化。 */
export async function setQuickAddCategory(cat: "work" | "life"): Promise<void> {
  ui.quickAddCategory = cat;
  try {
    await api.settingsSet(SETTING_KEYS.defaultCategory, cat);
  } catch (e) {
    console.error("分类记忆保存失败", e);
  }
}
