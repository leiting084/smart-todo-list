<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import TodoList from "../components/TodoList.svelte";
  import DateStrip from "../components/DateStrip.svelte";
  import CategoryToggle from "../components/CategoryToggle.svelte";
  import { api } from "../api";
  import {
    addTodo,
    loadToday,
    loadTodos,
    persistOrder,
    todosState,
  } from "../todos.svelte";
  import { ui } from "../stores.svelte";

  let draft = $state("");
  let toast = $state("");
  let timer: ReturnType<typeof setInterval> | undefined;
  let dateLoaded = $state(false);

  async function refresh(today: number) {
    if (ui.overdueMode) {
      // 逾期视图：拉全部未完成，再筛出日期早于今天的
      await loadTodos({ completed: false });
      const cutoff = todosState.today || today;
      todosState.items = todosState.items.filter((t) => t.date && t.date < cutoff);
    } else {
      await loadTodos({ date: today });
    }
    dateLoaded = true;
  }

  onMount(async () => {
    const today = await loadToday();
    // T3.6：从月总览定位到具体日期时，用该日替代"今天"
    if (ui.pendingDate) {
      const target = ui.pendingDate;
      ui.pendingDate = null;
      await refresh(target);
      return;
    }
    await refresh(today);
    // 逾期视图不挂跨天定时器（它本就不是"今天"的清单）
    if (ui.overdueMode) return;
    // 跨天：每 60 秒核对后端日期，0 点后自动指向新一天
    timer = setInterval(async () => {
      const now = await api.todayDate();
      if (now !== todosState.today) {
        todosState.today = now;
        // T3.5：0 点转点 → 生成重复实例 + 自动顺延逾期未完成
        await api.repeatGenerate().catch(() => {});
        await api.repeatPostpone().catch(() => {});
        await refresh(now);
      }
    }, 60_000);
  });
  onDestroy(() => timer && clearInterval(timer));

  async function submit(toInbox = false) {
    const text = draft;
    if (!text.trim()) return;
    draft = "";
    try {
      if (toInbox) {
        // Ctrl+Enter = 进收集箱（不在今日列表显示）
        await api.todoCreate({
          title: text.trim(),
          date: null,
          category: ui.quickAddCategory,
        });
        toast = "已加入收集箱";
        setTimeout(() => (toast = ""), 2000);
      } else {
        await addTodo(text, todosState.today, ui.quickAddCategory);
      }
    } catch (e) {
      draft = text;
      alert(`添加失败：${e}`);
    }
  }
</script>

<div class="today">
  {#if toast}
    <div class="toast">{toast}</div>
  {/if}
  <!-- 跨日期改期落点：把待办拖到某天即改期（External 同款操作） -->
  {#if dateLoaded}
    <DateStrip start={todosState.today} onMoved={() => refresh(todosState.today)} />
  {/if}
  <TodoList
    emptyText={dateLoaded
      ? ui.overdueMode
        ? "没有逾期的待办，干得漂亮。"
        : "今天还没有待办，下方添加一条吧。"
      : "加载中…"}
    onReorder={persistOrder}
    onRefresh={() => refresh(todosState.today)}
  />
  <div class="quick-add">
    <CategoryToggle />
    <input
      type="text"
      placeholder="添加待办（左侧选工作/生活，回车=今天，Ctrl+回车=收集箱）…"
      bind:value={draft}
      onkeydown={(e) => {
        if (e.key === "Enter") submit(e.ctrlKey || e.metaKey);
      }}
    />
    <button class="primary" onclick={() => submit(false)} disabled={!draft.trim()}>
      今天
    </button>
    <button onclick={() => submit(true)} disabled={!draft.trim()}>收集箱</button>
  </div>
</div>

<style>
  .today {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .toast {
    position: absolute;
    top: 60px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--color-primary);
    color: var(--color-primary-fg);
    padding: 6px 14px;
    border-radius: 16px;
    font-size: 12px;
    z-index: 20;
  }
  .quick-add {
    display: flex;
    gap: 8px;
    padding: 12px 18px;
    border-top: 1px solid var(--color-border);
    background: var(--color-surface);
  }
  .quick-add input {
    flex: 1;
  }
</style>
