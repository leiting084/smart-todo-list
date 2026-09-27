<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { api } from "../api";
  import SearchBar from "./SearchBar.svelte";
  import CategoryToggle from "./CategoryToggle.svelte";
  import { NAV_ITEMS, ui } from "../stores.svelte";
  import { toggleTheme } from "../theme";
  import { loadTags, removeTag, tagsState } from "../tags.svelte";

  let quickText = $state("");
  let miniOn = $state(false);

  onMount(loadTags);

  /** T3.2：开关迷你浮窗 */
  async function toggleMini() {
    try {
      const result = await invoke<string>("mini_window_toggle");
      miniOn = result !== "hidden";
      if (result === "hidden" || result === "created" || result === "shown") {
        // 浮窗呼出后检查是否创建成功（避免"沉默失败"）
        setTimeout(() => { miniOn = true; }, 500);
      }
    } catch (e) {
      alert(`浮窗操作失败：${e}`);
    }
  }

  // F2 快速添加：回车=今日待办，Ctrl+Enter=收集箱（侧栏常驻，跨视图可用）
  async function quickAdd(e: KeyboardEvent) {
    if (e.key !== "Enter" || !quickText.trim()) return;
    const text = quickText;
    quickText = "";
    try {
      const toInbox = e.ctrlKey || e.metaKey;
      await api.todoCreate({
        title: text.trim(),
        date: toInbox ? null : await api.todayDate(),
        category: ui.quickAddCategory,
      });
    } catch (err) {
      alert(`添加失败：${err}`);
    }
  }

  function go(itemId: (typeof NAV_ITEMS)[number]["id"]) {
    ui.view = itemId;
    ui.activeTag = null;
    // 任何侧栏导航都退出「逾期」视图（点「今天」回到正常今日清单）
    ui.overdueMode = false;
  }

  // NAV_ITEMS 已按分组连续排列，折叠成 {组名, 项[]} 用于渲染小标题
  interface NavGroup {
    name: string;
    items: typeof NAV_ITEMS;
  }
  const NAV_GROUPS: NavGroup[] = (() => {
    const out: NavGroup[] = [];
    for (const item of NAV_ITEMS) {
      const last = out[out.length - 1];
      if (last && last.name === item.group) last.items.push(item);
      else out.push({ name: item.group, items: [item] });
    }
    return out;
  })();

  async function onDeleteTag(id: string, name: string) {
    if (!window.confirm(`删除标签「${name}」？（不删除待办本身）`)) return;
    try {
      await removeTag(id);
      if (ui.activeTag === name) ui.activeTag = null;
    } catch (e) {
      alert(`删除失败：${e}`);
    }
  }
</script>

<aside class="sidebar">
  <nav class="nav">
    {#each NAV_GROUPS as group (group.name)}
      <div class="nav-group">
        <div class="group-title">{group.name}</div>
        {#each group.items as item (item.id)}
          <button
            class="nav-item"
            class:active={ui.view === item.id && !ui.activeTag}
            onclick={() => go(item.id)}
          >
            <span class="icon">{item.icon}</span>
            <span class="label">{item.label}</span>
          </button>
        {/each}
      </div>
    {/each}
  </nav>

  <!-- 标签区（动态；空标签自动隐藏，T1.3） -->
  <div class="tag-area">
    <div class="tag-area-title">标签</div>
    {#if tagsState.list.filter((t) => t.total > 0).length === 0}
      <div class="tag-empty">（暂无标签）</div>
    {:else}
      {#each tagsState.list.filter((t) => t.total > 0) as t (t.id)}
        <div
          class="tag-item"
          class:active={ui.activeTag === t.name}
          role="button"
          tabindex="0"
          onclick={() => (ui.activeTag = t.name)}
          onkeydown={(e) => e.key === "Enter" && (ui.activeTag = t.name)}
          title={`待办 ${t.todos} · 备忘 ${t.memos} · 笔记 ${t.notes}`}
        >
          <span class="tag-hash">#</span>
          <span class="tag-name">{t.name}</span>
          <span class="tag-count">{t.total}</span>
          <button
            class="tag-del"
            onclick={(e) => {
              e.stopPropagation();
              onDeleteTag(t.id, t.name);
            }}
            title="删除标签（不删除待办）"
          >
            ×
          </button>
        </div>
      {/each}
    {/if}
  </div>

  <SearchBar />

  <div class="sidebar-bottom">
    <!-- 快速添加：回车=今天 / Ctrl+Enter=收集箱；分类由下方切换决定 -->
    <div class="quick-cat">
      <span class="quick-cat-label">新待办</span>
      <CategoryToggle />
    </div>
    <input
      type="text"
      class="quick-input"
      placeholder="快速添加待办（回车=今天，Ctrl+回车=收集箱）"
      bind:value={quickText}
      onkeydown={quickAdd}
    />
    <button class="theme-toggle" onclick={toggleTheme} title="切换明/暗主题">
      {ui.theme === "dark" ? "☀ 浅色" : "🌙 深色"}
    </button>
    <button class="theme-toggle" onclick={toggleMini} title="开关迷你浮窗（T3.2）">
      {miniOn ? "关闭浮窗" : "打开浮窗"}
    </button>
  </div>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    background: var(--color-sidebar);
    border-right: 1px solid var(--color-border);
    min-width: 0;
    overflow: hidden;
  }

  .nav {
    display: flex;
    flex-direction: column;
    padding: 8px 6px;
    gap: 1px;
    overflow-y: auto;
  }

  .nav-group {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .group-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-text-dim);
    letter-spacing: 0.08em;
    padding: 8px 10px 3px;
    user-select: none;
  }
  .nav-group + .nav-group .group-title {
    border-top: 1px solid var(--color-border);
    margin-top: 6px;
    padding-top: 10px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 7px;
    padding: 7px 10px;
    color: var(--color-text);
  }
  .nav-item:hover {
    background: var(--color-hover);
    border-color: transparent;
  }
  .nav-item.active {
    background: var(--color-active);
    font-weight: 600;
    color: var(--color-primary);
  }
  .nav-item .icon {
    width: 18px;
    text-align: center;
    opacity: 0.85;
  }

  .tag-area {
    border-top: 1px solid var(--color-border);
    padding: 10px 12px;
    font-size: 12px;
    color: var(--color-text-dim);
    overflow-y: auto;
    flex: 1;
  }
  .tag-area-title {
    font-weight: 600;
    margin-bottom: 6px;
    color: var(--color-text);
  }
  .tag-empty {
    opacity: 0.6;
  }

  .tag-item {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 6px;
    padding: 3px 8px;
    margin: 1px 0;
    font-size: 12px;
    color: var(--color-text);
  }
  .tag-item:hover {
    background: var(--color-hover);
    border-color: transparent;
  }
  .tag-item.active {
    background: var(--color-active);
    color: var(--color-primary);
    font-weight: 600;
  }
  .tag-hash {
    opacity: 0.5;
  }
  .tag-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag-count {
    opacity: 0.55;
    font-size: 11px;
  }
  .tag-del {
    visibility: hidden;
    opacity: 0.5;
    padding: 0 3px;
  }
  .tag-item:hover .tag-del {
    visibility: visible;
  }
  .tag-del:hover {
    color: #dc2626;
    opacity: 1;
  }

  .sidebar-bottom {
    border-top: 1px solid var(--color-border);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .quick-cat {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .quick-cat-label {
    font-size: 12px;
    color: var(--color-text-dim);
  }
  .quick-input {
    width: 100%;
  }
  .theme-toggle {
    width: 100%;
  }
</style>
