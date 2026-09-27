<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Project, type ProjectStats, type Todo } from "../api";
  import {
    createProject,
    loadProjects,
    projectsState,
    setProjectArchived,
    updateProject,
  } from "../projects.svelte";

  let newName = $state("");
  let selectedId = $state<string | null>(null);
  let detailTodos = $state<Todo[]>([]);
  let stats = $state<ProjectStats | null>(null);
  const COLORS = ["red", "orange", "yellow", "green", "blue", "purple", "gray"];
  const COLOR_HEX: Record<string, string> = {
    red: "#ef4444", orange: "#f97316", yellow: "#eab308", green: "#22c55e",
    blue: "#3b82f6", purple: "#a855f7", gray: "#9ca3af",
  };
  // 详情页编辑态：颜色选择器是否展开、名称/描述草稿
  let colorPickerOpen = $state(false);
  let nameDraft = $state("");
  let descDraft = $state("");
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
    colorPickerOpen = false;
    nameDraft = p.name;
    descDraft = p.description ?? "";
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
  /** 选择项目颜色（null=清除），立即落库并收起选择器 */
  async function setColor(color: string | null) {
    if (!selected) return;
    colorPickerOpen = false;
    try {
      // 清除用空串 ""（不能传 null：JSON null 会被后端折叠成"不改"）
      await updateProject(selected.id, { color: color ?? "" });
    } catch (e) {
      alert(`更新颜色失败：${e}`);
    }
  }
  /** 提交名称；空或未变则还原草稿不落库 */
  async function commitName() {
    if (!selected) return;
    const name = nameDraft.trim();
    if (!name || name === selected.name) {
      nameDraft = selected.name;
      return;
    }
    try {
      await updateProject(selected.id, { name });
    } catch (e) {
      alert(`更新名称失败：${e}`);
      nameDraft = selected.name;
    }
  }
  /** 提交描述；清空 = 空串（后端映射为 NULL） */
  async function commitDesc() {
    if (!selected) return;
    const description = descDraft.trim();
    if (description === (selected.description ?? "")) return;
    try {
      await updateProject(selected.id, { description });
    } catch (e) {
      alert(`更新描述失败：${e}`);
    }
  }

  const activeTodos = $derived(detailTodos.filter((t) => !t.completed));
  const doneTodos = $derived(detailTodos.filter((t) => t.completed));
  const pct = $derived(Math.round((stats?.completionRate ?? 0) * 100));
</script>

{#if selected}
  <div class="detail">
    <button class="back" onclick={() => (selectedId = null)}>← 返回项目列表</button>
    <div class="detail-head">
      <span class="color-wrap">
        <button
          class="dot"
          style={selected.color ? `background:${COLOR_HEX[selected.color] ?? "#9ca3af"}` : ""}
          title="选择颜色"
          onclick={() => (colorPickerOpen = !colorPickerOpen)}
        ></button>
        {#if colorPickerOpen}
          <span class="color-pop">
            {#each COLORS as c (c)}
              <button
                class="swatch"
                style={`background:${COLOR_HEX[c]}`}
                onclick={() => setColor(c)}
                title={c}
              ></button>
            {/each}
            <button class="swatch none" onclick={() => setColor(null)} title="无颜色">∅</button>
          </span>
        {/if}
      </span>
      <input
        class="name-edit"
        type="text"
        bind:value={nameDraft}
        onkeydown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        onblur={commitName}
      />
      <button class="archive" onclick={() => archive(selected)}>归档</button>
    </div>
    <textarea
      class="desc-edit"
      placeholder="项目描述（可选）…"
      rows={2}
      bind:value={descDraft}
      onblur={commitDesc}
    ></textarea>

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
  .color-wrap { position: relative; display: inline-flex; flex: none; }
  button.dot { width: 16px; height: 16px; padding: 0; cursor: pointer; }
  .color-pop {
    position: absolute; top: 22px; left: 0; z-index: 20;
    display: flex; gap: 4px; background: var(--color-surface);
    border: 1px solid var(--color-border); border-radius: 6px; padding: 5px;
    box-shadow: var(--shadow);
  }
  .swatch { width: 16px; height: 16px; border-radius: 50%; border: 1px solid var(--color-border); padding: 0; cursor: pointer; }
  .swatch.none { background: var(--color-bg); font-size: 10px; line-height: 1; }
  .name-edit {
    flex: 1; min-width: 0; font-size: 18px; font-weight: 600; font-family: inherit;
    border: 1px solid transparent; background: transparent; border-radius: 6px; padding: 2px 6px;
  }
  .name-edit:hover, .name-edit:focus { border-color: var(--color-border); background: var(--color-surface); outline: none; }
  .desc-edit {
    width: 100%; box-sizing: border-box; margin-top: 6px; font-size: 13px; font-family: inherit;
    color: var(--color-text-dim); border: 1px solid transparent; background: transparent;
    border-radius: 6px; padding: 4px 6px; resize: vertical;
  }
  .desc-edit:hover, .desc-edit:focus { border-color: var(--color-border); background: var(--color-surface); outline: none; }
  .progress { display: flex; align-items: center; gap: 10px; margin: 12px 0; }
  .bar { flex: 1; height: 8px; background: var(--color-border); border-radius: 4px; overflow: hidden; }
  .fill { height: 100%; background: var(--color-primary); }
  .pct { font-size: 12px; color: var(--color-text-dim); white-space: nowrap; }
  h3 { font-size: 13px; margin: 14px 0 6px; }
  .todos { list-style: none; margin: 0; padding: 0; }
  .todos li { display: flex; gap: 8px; padding: 4px 0; }
  .todos.done span { text-decoration: line-through; color: var(--color-text-dim); }
</style>
