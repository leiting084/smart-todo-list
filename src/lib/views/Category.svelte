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
  let debounce: ReturnType<typeof setTimeout> | undefined;

  const isAll = $derived(category === undefined);
  // 排序方式："" = 手动拖拽序；"priority" = 按优先级。全部视图默认按优先级。
  // Category 由 +page.svelte 以 paneKey=key 挂载，切换 work/life/all 会重建组件，
  // 故 category 在组件生命周期内恒定；这里就是要读它的初始值。
  // svelte-ignore state_referenced_locally
  let sort = $state(category === undefined ? "priority" : "");
  // 新建待办落到的分类：固定分类视图用 prop，全部视图用共享开关。
  const newCategory = $derived(category ?? ui.quickAddCategory);

  function apply() {
    loadTodos({
      category,
      priority: (priority || undefined) as Todo["priority"] | undefined,
      q: q.trim() || undefined,
      sort: (sort || undefined) as "priority" | undefined,
    });
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

  <TodoList
    emptyText={q || priority
      ? "没有符合条件的待办。"
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
