<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Link } from "../api";
  import { archiveMemo, loadMemos, memosState, updateMemo } from "../memos.svelte";
  import { loadNotes, notesState } from "../notes.svelte";

  let editingId = $state<string | null>(null);
  let contentDraft = $state("");
  let sources = $state<Map<string, string>>(new Map()); // memoId -> 来源 todo 标题

  // 整理为笔记（T2.3）
  let selected = $state<Set<string>>(new Set());
  let showOrganize = $state(false);
  let organizeMode = $state<"new" | "append">("new");
  let newTitle = $state("");
  let appendTarget = $state<string>("");

  onMount(() => loadMemos());

  async function loadSourceTitles() {
    const next = new Map<string, string>();
    for (const m of memosState.list) {
      const links: Link[] = await api.linksForEntity("memo", m.id).catch(() => []);
      const src = links.find((l) => l.relation === "derived_from" && l.fromId === m.id);
      if (src) {
        const todo = await api.todoGet(src.toId).catch(() => null);
        next.set(m.id, todo?.title ?? "（来源待办已删除）");
      }
    }
    sources = next;
  }
  $effect(() => {
    void memosState.list.length;
    void loadSourceTitles();
  });

  function startEdit(id: string, content: string) {
    editingId = id;
    contentDraft = content ?? "";
  }
  async function commitEdit(id: string) {
    if (editingId !== id) return;
    editingId = null;
    try {
      await updateMemo(id, { content: contentDraft || null });
    } catch (e) {
      alert(`保存失败：${e}`);
    }
  }
  async function onDelete(id: string) {
    if (!window.confirm("删除该备忘？（归档，可在导入数据中找回）")) return;
    await archiveMemo(id);
  }

  function toggleSelect(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  async function openOrganize() {
    await loadNotes();
    organizeMode = "new";
    newTitle = "";
    appendTarget = "";
    showOrganize = true;
  }
  async function doOrganize() {
    if (!selected.size) return;
    const noteId = organizeMode === "append" ? appendTarget : null;
    const title = organizeMode === "new" ? newTitle.trim() : "";
    if (organizeMode === "new" && !title) return;
    if (organizeMode === "append" && !appendTarget) return;
    try {
      const note = await api.memoToNote([...selected], noteId, title);
      showOrganize = false;
      selected = new Set();
      await loadMemos();
      const go = window.confirm(`已${noteId ? "追加" : "整理"}到笔记「${note.title}」，打开编辑？`);
      if (go) window.dispatchEvent(new CustomEvent("open-note", { detail: note.id }));
    } catch (e) {
      alert(`整理失败：${e}`);
    }
  }

  const fmt = (ms: number) => {
    const d = new Date(ms);
    return `${d.getFullYear()}-${`${d.getMonth() + 1}`.padStart(2, "0")}-${`${d.getDate()}`.padStart(2, "0")}`;
  };
</script>

<div class="memos">
  <div class="actions">
    <button class="primary" onclick={openOrganize} disabled={selected.size === 0}>
      整理为笔记（{selected.size}）
    </button>
  </div>

  {#if showOrganize}
    <div class="organize">
      <label class="radio">
        <input type="radio" checked bind:group={organizeMode} value="new" /> 新建笔记
        <input
          type="text"
          placeholder="新笔记标题…"
          bind:value={newTitle}
          disabled={organizeMode !== "new"}
        />
      </label>
      <label class="radio">
        <input type="radio" bind:group={organizeMode} value="append" /> 追加到已有笔记
        <select bind:value={appendTarget} disabled={organizeMode !== "append"}>
          <option value="">选择笔记…</option>
          {#each notesState.list as n (n.id)}
            <option value={n.id}>{n.title}</option>
          {/each}
        </select>
      </label>
      <p class="hint">将生成 Markdown：每条备忘一个小节（`## 日期 标题`），备忘与笔记建立双向链接。</p>
      <div class="org-actions">
        <button class="primary" onclick={doOrganize}>执行整理</button>
        <button onclick={() => (showOrganize = false)}>取消</button>
      </div>
    </div>
  {/if}

  {#if memosState.list.length === 0}
    <p class="empty">还没有备忘。完成一条待办时点「转备忘」，事后记录会出现在这里。</p>
  {/if}
  <div class="cards">
    {#each memosState.list as m (m.id)}
      <article class="memo" class:picked={selected.has(m.id)}>
        <header>
          <input
            type="checkbox"
            checked={selected.has(m.id)}
            onchange={() => toggleSelect(m.id)}
            title="选中以便整理为笔记"
          />
          <h3>{m.title}</h3>
          <span class="date">{fmt(m.createdAt)}</span>
        </header>
        {#if sources.has(m.id)}
          <p class="source">来自待办：{sources.get(m.id)}</p>
        {/if}
        {#if editingId === m.id}
          <textarea
            bind:value={contentDraft}
            rows="4"
            onblur={() => commitEdit(m.id)}
            onkeydown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) commitEdit(m.id);
            }}
          ></textarea>
          <p class="hint">失焦或 Ctrl+Enter 保存</p>
        {:else}
          <div
            class="content"
            role="button"
            tabindex="0"
            onclick={() => startEdit(m.id, m.content ?? "")}
            onkeydown={(e) => e.key === "Enter" && startEdit(m.id, m.content ?? "")}
            title="点击编辑"
          >
            {m.content || "（无内容，点击补充）"}
          </div>
        {/if}
        {#if m.tags.length}
          <div class="tags">
            {#each m.tags as t (t)}<span class="pill">#{t}</span>{/each}
          </div>
        {/if}
        <footer>
          <button class="mini" onclick={() => startEdit(m.id, m.content ?? "")}>编辑</button>
          <button class="mini del" onclick={() => onDelete(m.id)}>删除</button>
        </footer>
      </article>
    {/each}
  </div>
</div>

<style>
  .memos { padding: 10px 18px 14px; overflow-y: auto; flex: 1; }
  .actions { margin-bottom: 10px; }
  .organize { background: var(--color-surface); border: 1px solid var(--color-primary); border-radius: 8px; padding: 12px; margin-bottom: 12px; display: flex; flex-direction: column; gap: 8px; }
  .radio { display: flex; align-items: center; gap: 8px; font-size: 13px; flex-wrap: wrap; }
  .radio input[type="text"], .radio select { flex: 1; min-width: 160px; }
  .hint { font-size: 11px; color: var(--color-text-dim); margin: 0; }
  .org-actions { display: flex; gap: 8px; }
  .empty { color: var(--color-text-dim); text-align: center; margin-top: 10vh; }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; }
  .memo { background: var(--color-surface); border: 1px solid var(--color-border); border-radius: 8px; padding: 12px; display: flex; flex-direction: column; gap: 6px; }
  .memo.picked { border-color: var(--color-primary); }
  header { display: flex; align-items: center; gap: 8px; }
  h3 { margin: 0; font-size: 14px; flex: 1; min-width: 0; }
  .date { font-size: 11px; color: var(--color-text-dim); white-space: nowrap; }
  .source { font-size: 11px; color: var(--color-primary); margin: 0; }
  .content { font-size: 13px; margin: 0; white-space: pre-wrap; cursor: text; min-height: 1.4em; }
  div.content { outline: none; }
  div.content:focus-visible { border: 1px solid var(--color-primary); border-radius: 4px; }
  textarea { width: 100%; font-family: inherit; font-size: 13px; padding: 6px; border: 1px solid var(--color-border); border-radius: 6px; background: var(--color-bg); color: var(--color-text); resize: vertical; }
  .tags { display: flex; gap: 4px; flex-wrap: wrap; }
  .pill { font-size: 11px; padding: 0 7px; border-radius: 10px; border: 1px solid var(--color-primary); color: var(--color-primary); background: var(--color-primary-soft); }
  footer { display: flex; gap: 6px; }
  .mini { font-size: 12px; padding: 2px 8px; }
  .del { color: #dc2626; }
</style>
