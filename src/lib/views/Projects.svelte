<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Project, type ProjectStats, type Todo } from "../api";
  import {
    createProject,
    loadProjects,
    projectsState,
    setProjectArchived,
  } from "../projects.svelte";

  let newName = $state("");
  let selectedId = $state<string | null>(null);
  let detailTodos = $state<Todo[]>([]);
  let stats = $state<ProjectStats | null>(null);
  const COLOR_HEX: Record<string, string> = {
    red: "#ef4444", orange: "#f97316", yellow: "#eab308", green: "#22c55e",
    blue: "#3b82f6", purple: "#a855f7", gray: "#9ca3af",
  };
  const selected = $derived(projectsState.list.find((p) => p.id === selectedId) ?? null);

  onMount(() => loadProjects());

  async function add() {
    const name = newName.trim();
    if (!name) return;
    newName = "";
    try {
      await createProject(name);
    } catch (e) {
      alert(`创建失败：${e}`);
    }
  }

  async function open(p: Project) {
    selectedId = p.id;
    await refreshDetail();
  }
  async function refreshDetail() {
    if (!selectedId) return;
    const [todos, s] = await Promise.all([
      api.todoList({ projectId: selectedId }),
      api.projectStats(selectedId),
    ]);
    detailTodos = todos;
    stats = s;
  }
  async function toggle(t: Todo) {
    await api.todoSetCompleted(t.id, !t.completed);
    await refreshDetail();
  }
  async function archive(p: Project) {
    if (!window.confirm(`归档项目「${p.name}」？其待办仍保留。`)) return;
    await setProjectArchived(p.id, true);
    selectedId = null;
  }

  const activeTodos = $derived(detailTodos.filter((t) => !t.completed));
  const doneTodos = $derived(detailTodos.filter((t) => t.completed));
  const pct = $derived(Math.round((stats?.completionRate ?? 0) * 100));
</script>

{#if selected}
  <div class="detail">
    <button class="back" onclick={() => (selectedId = null)}>← 返回项目列表</button>
    <div class="detail-head">
      <span
        class="dot"
        style={selected.color ? `background:${COLOR_HEX[selected.color] ?? "#9ca3af"}` : ""}
      ></span>
      <h2>{selected.name}</h2>
      <button class="archive" onclick={() => archive(selected)}>归档</button>
    </div>
    {#if selected.description}
      <p class="desc">{selected.description}</p>
    {/if}

    <div class="progress">
      <div class="bar"><div class="fill" style={`width:${pct}%`}></div></div>
      <span class="pct">{stats?.completedCount}/{stats?.todoCount}（{pct}%）</span>
    </div>

    <h3>待办 · 未完成（{activeTodos.length}）</h3>
    <ul class="todos">
      {#each activeTodos as t (t.id)}
        <li>
          <input type="checkbox" checked={t.completed} onchange={() => toggle(t)} />
          <span>{t.title}</span>
        </li>
      {/each}
    </ul>
    <h3>已完成（{doneTodos.length}）</h3>
    <ul class="todos done">
      {#each doneTodos as t (t.id)}
        <li>
          <input type="checkbox" checked={t.completed} onchange={() => toggle(t)} />
          <span>{t.title}</span>
        </li>
      {/each}
    </ul>
  </div>
{:else}
  <div class="list">
    <div class="add">
      <input type="text" placeholder="新建项目名称…" bind:value={newName}
        onkeydown={(e) => e.key === "Enter" && add()} />
      <button class="primary" onclick={add} disabled={!newName.trim()}>新建项目</button>
    </div>
    {#if projectsState.list.length === 0}
      <p class="empty">还没有项目。项目用于归集工作任务（继承旧版）。</p>
    {/if}
    <div class="cards">
      {#each projectsState.list as p (p.id)}
        <button class="card" onclick={() => open(p)}>
          <span class="card-top">
            <span class="dot" style={p.color ? `background:${COLOR_HEX[p.color] ?? "#9ca3af"}` : ""}></span>
            <span class="name">{p.name}</span>
          </span>
          {#if p.description}<span class="card-desc">{p.description}</span>{/if}
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .list, .detail { padding: 14px 18px; overflow-y: auto; flex: 1; }
  .add { display: flex; gap: 8px; margin-bottom: 14px; }
  .add input { flex: 1; }
  .empty { color: var(--color-text-dim); }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 10px; }
  .card {
    text-align: left; padding: 12px; display: flex; flex-direction: column; gap: 6px;
    background: var(--color-surface);
  }
  .card-top { display: flex; align-items: center; gap: 8px; font-weight: 600; }
  .card-desc { font-size: 12px; color: var(--color-text-dim); }
  .dot { width: 12px; height: 12px; border-radius: 50%; border: 1px solid var(--color-border); display: inline-block; }
  .back { margin-bottom: 10px; }
  .detail-head { display: flex; align-items: center; gap: 10px; }
  .detail-head h2 { margin: 0; font-size: 18px; flex: 1; }
  .desc { color: var(--color-text-dim); font-size: 13px; }
  .progress { display: flex; align-items: center; gap: 10px; margin: 12px 0; }
  .bar { flex: 1; height: 8px; background: var(--color-border); border-radius: 4px; overflow: hidden; }
  .fill { height: 100%; background: var(--color-primary); }
  .pct { font-size: 12px; color: var(--color-text-dim); white-space: nowrap; }
  h3 { font-size: 13px; margin: 14px 0 6px; }
  .todos { list-style: none; margin: 0; padding: 0; }
  .todos li { display: flex; gap: 8px; padding: 4px 0; }
  .todos.done span { text-decoration: line-through; color: var(--color-text-dim); }
</style>
