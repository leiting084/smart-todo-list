<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Person } from "../api";
  import { createPerson, loadPeople, peopleState, setPersonArchived } from "../people.svelte";

  let newName = $state("");
  let newRole = $state("");
  let newEmail = $state("");
  let counts = $state<Record<string, { total: number; done: number }>>({});

  // 行内编辑态（编辑姓名/职责/邮箱/备注）
  let editingId = $state<string | null>(null);
  let editName = $state("");
  let editRole = $state("");
  let editEmail = $state("");
  let editNote = $state("");

  onMount(async () => {
    await loadPeople();
    await refreshCounts();
  });

  async function refreshCounts() {
    const next: Record<string, { total: number; done: number }> = {};
    for (const p of peopleState.list) {
      const todos = await api.todoList({ assignee: p.id });
      next[p.id] = { total: todos.length, done: todos.filter((t) => t.completed).length };
    }
    counts = next;
  }

  async function add() {
    const name = newName.trim();
    if (!name) return;
    const role = newRole.trim() || null;
    const email = newEmail.trim() || null;
    newName = "";
    newRole = "";
    newEmail = "";
    try {
      await createPerson(name, role, email);
      await refreshCounts();
    } catch (e) {
      alert(`创建失败：${e}`);
    }
  }

  function startEdit(p: Person) {
    editingId = p.id;
    editName = p.name;
    editRole = p.role ?? "";
    editEmail = p.email ?? "";
    editNote = p.note ?? "";
  }

  function cancelEdit() {
    editingId = null;
  }

  async function saveEdit(p: Person) {
    const name = editName.trim();
    if (!name) return;
    try {
      const updated = await api.personUpdate(p.id, {
        name,
        role: editRole.trim() || null,
        email: editEmail.trim() || null,
        note: editNote.trim() || null,
      });
      peopleState.list = peopleState.list.map((x) => (x.id === updated.id ? updated : x));
      editingId = null;
      await refreshCounts();
    } catch (e) {
      alert(`保存失败：${e}`);
    }
  }

  async function archive(p: Person) {
    if (!window.confirm(`归档人员「${p.name}」？历史分配记录保留。`)) return;
    await setPersonArchived(p.id, true);
    await refreshCounts();
  }
</script>

<div class="people">
  <div class="add">
    <input
      type="text"
      placeholder="姓名…（本地责任人清单，非账号体系）"
      bind:value={newName}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <input
      class="opt"
      type="text"
      placeholder="职责（可选，如：前端）"
      bind:value={newRole}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <input
      class="opt"
      type="text"
      placeholder="邮箱（可选）"
      bind:value={newEmail}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <button class="primary" onclick={add} disabled={!newName.trim()}>新建人员</button>
  </div>

  {#if peopleState.list.length === 0}
    <p class="empty">还没有人员。工作任务可分配给多人协作完成。</p>
  {/if}

  <ul class="list">
    {#each peopleState.list as p (p.id)}
      <li class="person">
        {#if editingId === p.id}
          <div class="edit-form">
            <input placeholder="姓名" bind:value={editName} />
            <input placeholder="职责" bind:value={editRole} />
            <input placeholder="邮箱" bind:value={editEmail} />
            <input placeholder="备注" bind:value={editNote} />
            <button class="primary" onclick={() => saveEdit(p)} disabled={!editName.trim()}>保存</button>
            <button onclick={cancelEdit}>取消</button>
          </div>
        {:else}
          <div class="info">
            <span class="name">{p.name}</span>
            {#if p.role}<span class="role">{p.role}</span>{/if}
            {#if p.email}<span class="email">{p.email}</span>{/if}
            {#if p.note}<span class="note">{p.note}</span>{/if}
          </div>
          <span class="stat">
            {#if counts[p.id]}
              负责任务 {counts[p.id].total} · 完成 {counts[p.id].done}
            {:else}
              …
            {/if}
          </span>
          <button class="edit" onclick={() => startEdit(p)}>编辑</button>
          <button class="archive" onclick={() => archive(p)}>归档</button>
        {/if}
      </li>
    {/each}
  </ul>
</div>

<style>
  .people { padding: 14px 18px; overflow-y: auto; flex: 1; }
  .add { display: flex; gap: 8px; margin-bottom: 14px; flex-wrap: wrap; }
  .add input { flex: 1; min-width: 140px; }
  .add input.opt { flex: 0.7; }
  .empty { color: var(--color-text-dim); }
  .list { list-style: none; margin: 0; padding: 0; }
  .person {
    display: flex; align-items: center; gap: 12px;
    padding: 10px 12px; border: 1px solid var(--color-border);
    border-radius: 8px; margin-bottom: 8px; background: var(--color-surface);
  }
  .info { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap; }
  .name { font-weight: 600; }
  .role {
    font-size: 11px; line-height: 1.4; padding: 0 7px; border-radius: 10px;
    border: 1px solid var(--color-primary); color: var(--color-primary);
    background: var(--color-primary-soft); white-space: nowrap;
  }
  .email { font-size: 12px; color: var(--color-text-dim); }
  .note { font-size: 12px; color: var(--color-text-dim); }
  .edit-form { flex: 1; display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .edit-form input { flex: 1; min-width: 110px; font-size: 13px; }
  .edit-form button { font-size: 12px; }
  .stat { font-size: 12px; color: var(--color-text-dim); white-space: nowrap; }
  .edit, .archive { font-size: 12px; }
</style>
