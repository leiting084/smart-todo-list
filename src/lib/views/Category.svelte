<script lang="ts">
  import { onMount } from "svelte";
  import TodoList from "../components/TodoList.svelte";
  import CategoryToggle from "../components/CategoryToggle.svelte";
  import { api } from "../api";
  import { ui } from "../stores.svelte";
  import {
    loadTodos,
    loadToday,
    persistOrder,
    todosState,
  } from "../todos.svelte";
  import type { Todo } from "../api";

  // category 省略（undefined）时即「全部待办」视图：不按分类过滤，快速添加用共享的分类开关。
  let {
    category,
  }: { category?: Extract<Todo["category"], "work" | "life"> } = $props();

  let priority = $state(""); // "" = 全部
  let q = $state("");
  let draft = $state("");
  // 完成状态：active=未完成 / done=已完成 / all=全部。默认只看未完成（列表太长时聚焦待办）。
  let status = $state<"active" | "done" | "all">("active");
  // 统计：当前筛选域内、与所选 status 相反那一侧的条数（active 时=已完成数，done 时=未完成数）。
  let otherCount = $state(0);
  let debounce: ReturnType<typeof setTimeout> | undefined;

  const isAll = $derived(category === undefined);
  // 排序方式："" = 手动拖拽序；"priority" = 按优先级。全部视图默认按优先级。
  // Category 由 +page.svelte 以 paneKey=key 挂载，切换 work/life/all 会重建组件，
  // 故 category 在组件生命周期内恒定；这里就是要读它的初始值。
  // svelte-ignore state_referenced_locally
  let sort = $state(category === undefined ? "priority" : "");
  // 新建待办落到的分类：固定分类视图用 prop，全部视图用共享开关。
  const newCategory = $derived(category ?? ui.quickAddCategory);

  /** 与当前筛选同域（category/priority/q），但忽略 status 切换，取反再查一次拿另一半计数。 */
  async function loadStats() {
    if (status === "all") {
      otherCount = 0; // 全部时直接由列表分拆，无需额外查询
      return;
    }
    const opposite = status === "active"; // active→查已完成；done→查未完成
    try {
      const list = await api.todoList({
        category,
        priority: (priority || undefined) as Todo["priority"] | undefined,
        q: q.trim() || undefined,
        completed: opposite,
      });
      otherCount = list.length;
    } catch {
      otherCount = 0;
    }
  }

  const completedCount = $derived(
    status === "all"
      ? todosState.items.filter((t) => t.completed).length
      : status === "done"
        ? todosState.items.filter((t) => t.completed).length
        : otherCount + todosState.items.filter((t) => t.completed).length,
  );
  const uncompletedCount = $derived(
    status === "all"
      ? todosState.items.filter((t) => !t.completed).length
      : status === "active"
        ? todosState.items.filter((t) => !t.completed).length
        : otherCount + todosState.items.filter((t) => !t.completed).length,
  );
  const totalCount = $derived(completedCount + uncompletedCount);
  const doneRate = $derived(totalCount ? Math.round((completedCount / totalCount) * 100) : 0);

  async function apply() {
    await loadTodos({
      category,
      priority: (priority || undefined) as Todo["priority"] | undefined,
      q: q.trim() || undefined,
      sort: (sort || undefined) as "priority" | undefined,
      completed: status === "all" ? undefined : status === "done",
    });
    await loadStats();
  }

  onMount(async () => {
    await loadToday();
    apply();
  });

  function onSearch(value: string) {
    q = value;
    clearTimeout(debounce);
    debounce = setTimeout(apply, 250);
  }

  async function add() {
    const text = draft.trim();
    if (!text) return;
    draft = "";
    try {
      await api.todoCreate({ title: text, date: todosState.today, category: newCategory });
      apply();
    } catch (e) {
      draft = text;
      alert(`添加失败：${e}`);
    }
  }
</script>

<div class="pane">
  <div class="filters">
    <div class="seg" role="group" aria-label="完成状态">
      <button class="seg-btn" class:seg-on={status === "active"} onclick={() => { status = "active"; apply(); }}>未完成</button>
      <button class="seg-btn" class:seg-on={status === "done"} onclick={() => { status = "done"; apply(); }}>已完成</button>
      <button class="seg-btn" class:seg-on={status === "all"} onclick={() => { status = "all"; apply(); }}>全部</button>
    </div>
    <input
      type="text"
      placeholder="搜索标题 / 描述 / 标签…"
      value={q}
      oninput={(e) => onSearch((e.target as HTMLInputElement).value)}
    />
    <select value={priority} onchange={(e) => { priority = (e.target as HTMLSelectElement).value; apply(); }}>
      <option value="">全部优先级</option>
      <option value="high">高</option>
      <option value="medium">中</option>
      <option value="low">低</option>
    </select>
    <select value={sort} onchange={(e) => { sort = (e.target as HTMLSelectElement).value; apply(); }} title="列表排序方式">
      <option value="">手动排序</option>
      <option value="priority">按优先级</option>
    </select>
  </div>

  {#if totalCount > 0}
    <div class="stats">
      <span class="st-item st-undo">未完成 <b>{uncompletedCount}</b></span>
      <span class="dot">·</span>
      <span class="st-item st-done">已完成 <b>{completedCount}</b></span>
      <span class="dot">·</span>
      <span class="st-item st-total">共 <b>{totalCount}</b> 条</span>
      <span class="st-spacer"></span>
      <span class="st-rate">完成率 {doneRate}%</span>
      <span class="bar" title={`已完成 ${completedCount} / ${totalCount}`}><span class="bar-fill" style={`width:${doneRate}%`}></span></span>
    </div>
  {/if}

  <TodoList
    emptyText={q || priority
      ? "没有符合条件的待办。"
      : status === "active"
        ? "没有未完成的任务，全部搞定 🎉"
        : status === "done"
          ? "还没有已完成的待办。"
          : isAll
            ? "还没有任何待办。"
            : "该分类还没有待办。"}
    onReorder={persistOrder}
    onRefresh={apply}
    reorderable={sort !== "priority"}
  />

  <div class="quick-add">
    {#if isAll}
      <CategoryToggle />
    {/if}
    <input
      type="text"
      placeholder={isAll
        ? "添加待办（左侧选工作/生活，回车=今天）…"
        : `添加一条${category === "work" ? "工作" : "生活"}待办（回车=今天）…`}
      bind:value={draft}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <button class="primary" onclick={add} disabled={!draft.trim()}>添加</button>
  </div>
</div>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .filters {
    display: flex;
    gap: 8px;
    padding: 10px 18px;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }
  .filters input {
    flex: 1;
  }
  .seg {
    display: inline-flex;
    flex-shrink: 0;
    border: 1px solid var(--color-border);
    border-radius: 7px;
    overflow: hidden;
  }
  .seg-btn {
    border: none;
    background: var(--color-bg);
    color: var(--color-text-dim);
    font-size: 12px;
    padding: 4px 12px;
    cursor: pointer;
    border-radius: 0;
  }
  .seg-btn + .seg-btn {
    border-left: 1px solid var(--color-border);
  }
  .seg-btn:hover {
    color: var(--color-text);
  }
  .seg-btn.seg-on {
    background: var(--color-primary);
    color: #fff;
    font-weight: 600;
  }
  .stats {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 18px;
    font-size: 12px;
    color: var(--color-text-dim);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg);
  }
  .stats b {
    color: var(--color-text);
    font-weight: 700;
  }
  .st-undo b {
    color: var(--color-primary);
  }
  .st-done b {
    color: #16a34a;
  }
  .stats .dot {
    opacity: 0.5;
  }
  .st-spacer {
    flex: 1;
  }
  .st-rate {
    font-variant-numeric: tabular-nums;
  }
  .bar {
    display: inline-block;
    width: 90px;
    height: 6px;
    border-radius: 3px;
    background: var(--color-border);
    overflow: hidden;
  }
  .bar-fill {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: #16a34a;
    transition: width 0.25s ease;
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
