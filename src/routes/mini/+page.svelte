<script lang="ts">
  // F12 迷你浮窗 v2：默认"长条"（今日未完成计数+快速添加），点击展开今日列表。
  // 目标：小、置顶、不打断当前工作（SPEC §2 对齐 External）。
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import { api, type Todo, type MiniSettings } from "$lib/api";

  let items = $state<Todo[]>([]);
  let draft = $state("");
  let today = $state(0);
  let expanded = $state(false);
  let unlisteners: UnlistenFn[] = [];
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let win = getCurrentWindow();

  // F12 补齐：置底/透明度/贴边隐藏。窗口已 transparent(true)，透明度由根容器 opacity 承担。
  let mini = $state<MiniSettings>({ pin_mode: "top", opacity: 1, edge_hide: false });
  let lastCollapse = 0;

  async function loadMiniSettings() {
    try {
      mini = await api.miniGetSettings();
    } catch {
      /* 后端不可用时保持默认 */
    }
  }

  /** 鼠标移出且开启贴边隐藏 → 缩成边条贴在最近屏幕边缘 */
  async function onLeave() {
    if (!mini.edge_hide) return;
    try {
      await api.miniEdgeCollapse();
      lastCollapse = Date.now();
    } catch {
      /* 忽略：浮窗可能正在关闭 */
    }
  }

  /** 鼠标移入 → 展开。刚缩回的 400ms 内忽略，否则鼠标仍在原位置会立刻展开造成抖动 */
  async function onEnter() {
    if (!mini.edge_hide) return;
    if (Date.now() - lastCollapse < 400) return;
    try {
      await api.miniEdgeExpand();
    } catch {
      /* 忽略 */
    }
  }

  const open = $derived(items.length);

  async function refresh() {
    today = await invoke("today_date");
    items = (await api.todoList({ date: today, completed: false })) as Todo[];
  }

  async function toggle(t: Todo) {
    await api.todoSetCompleted(t.id, !t.completed);
    await refresh();
    await notifyMain();
  }

  async function add() {
    const text = draft.trim();
    if (!text) return;
    draft = "";
    await api.todoCreate({ title: text, date: today });
    await refresh();
    await notifyMain();
  }

  /** 展开时把窗口从长条拉大；折叠时拉回长条（尺寸变化由窗口端持久化防抖） */
  async function setExpanded(next: boolean) {
    expanded = next;
    const [w, h] = next ? [240, 300] : [220, 64];
    await win.setSize(new LogicalSize(w, h));
    scheduleSaveBounds();
  }

  async function notifyMain() {
    try {
      const { emit } = await import("@tauri-apps/api/event");
      await emit("todos-changed");
    } catch {
      /* ignore */
    }
  }

  function scheduleSaveBounds() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      try {
        const [x, y] = await invoke<[number, number]>("mini_window_get_pos");
        const [width, height] = await invoke<[number, number]>("mini_window_get_size");
        await invoke("mini_window_save_bounds", { x, y, width, height });
      } catch {
        /* 窗口正在关闭 */
      }
    }, 800);
  }

  onMount(async () => {
    await refresh();
    await loadMiniSettings();
    // 设置页改动后热更新（无需重启浮窗）
    unlisteners.push(
      await listen<MiniSettings>("mini-settings-changed", (e) => (mini = e.payload)),
    );
    unlisteners.push(await listen("todos-changed", () => void refresh()));
    const unMoved = await win.onMoved(() => scheduleSaveBounds());
    const unResized = await win.onResized(() => scheduleSaveBounds());
    unlisteners.push(unMoved, unResized);
  });
  onDestroy(() => {
    clearTimeout(saveTimer);
    unlisteners.forEach((u) => u());
  });
</script>

<div
  class="root"
  role="group"
  aria-label="迷你浮窗"
  style="opacity: {mini.opacity}"
  onmouseenter={onEnter}
  onmouseleave={onLeave}
>
{#if expanded}
  <div class="mini expanded">
    <header>
      <button class="fold" onclick={() => setExpanded(false)} title="收起">▾</button>
      <span class="title">今日（{open}）</span>
      <button class="close" onclick={() => win.hide()} title="隐藏">✕</button>
    </header>
    <ul>
      {#each items as t (t.id)}
        <li>
          <input type="checkbox" checked={t.completed} onchange={() => toggle(t)} />
          <span class="t">{t.title}</span>
        </li>
      {/each}
      {#if open === 0}<li class="empty">今日全部完成 🎉</li>{/if}
    </ul>
    <div class="quick">
      <input type="text" placeholder="快速添加…" bind:value={draft}
        onkeydown={(e) => e.key === "Enter" && add()} />
    </div>
  </div>
{:else}
  <div
    class="bar"
    role="button"
    tabindex="0"
    onclick={() => setExpanded(true)}
    onkeydown={(e) => (e.key === "Enter" || e.key === " ") && setExpanded(true)}
    title="点击展开今日列表"
  >
    <span class="dot" class:done={open === 0}></span>
    <span class="bar-text">{open === 0 ? "今日全部完成" : `今日待办 ${open} 条`}</span>
    <button class="close" onclick={(e) => { e.stopPropagation(); win.hide(); }} title="隐藏">✕</button>
  </div>
{/if}
</div>

<style>
  :global(html, body) { background: transparent; overflow: hidden; }
  /* 根容器：承担窗口透明度（窗口本身 transparent），并挂贴边隐藏的进出事件 */
  .root { height: 100%; }
  .bar {
    height: 64px;
    padding: 0 12px;
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--color-primary);
    color: var(--color-primary-fg);
    cursor: pointer;
    user-select: none;
  }
  .dot { width: 10px; height: 10px; border-radius: 50%; background: #fff; flex-shrink: 0; }
  .dot.done { background: var(--color-primary-soft); }
  .bar-text { flex: 1; font-size: 13px; font-weight: 600; white-space: nowrap; }
  .mini { display: flex; flex-direction: column; height: 100%; background: var(--color-surface); color: var(--color-text); }
  header { display: flex; align-items: center; padding: 8px 10px; border-bottom: 1px solid var(--color-border); }
  .fold, .close { border: none; background: transparent; color: var(--color-text-dim); padding: 0 4px; }
  .title { flex: 1; font-weight: 600; font-size: 13px; text-align: center; }
  ul { list-style: none; margin: 0; padding: 4px 0; flex: 1; overflow-y: auto; }
  li { display: flex; align-items: center; gap: 8px; padding: 6px 12px; font-size: 13px; }
  .t { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { color: var(--color-text-dim); justify-content: center; }
  .quick { border-top: 1px solid var(--color-border); padding: 8px 10px; }
  .quick input { width: 100%; }
</style>