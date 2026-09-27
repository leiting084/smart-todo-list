<script lang="ts">
  import { onMount } from "svelte";
  import NoteEditor from "../components/NoteEditor.svelte";
  import { createNote, deleteNote, loadNotes, notesState, renameNote } from "../notes.svelte";

  let openId = $state<string | null>(null);
  let newTitle = $state("");

  onMount(() => {
    void loadNotes();
    // T2.3：整理完成后的"打开编辑"跳转
    const onOpen = (e: Event) => {
      openId = (e as CustomEvent<string>).detail;
    };
    window.addEventListener("open-note", onOpen);
    return () => window.removeEventListener("open-note", onOpen);
  });

  async function add() {
    const title = newTitle.trim();
    if (!title) return;
    newTitle = "";
    try {
      const n = await createNote(title);
      openId = n.id;
    } catch (e) {
      alert(`创建失败：${e}`);
    }
  }

  async function remove(id: string, title: string) {
    if (!window.confirm(`删除笔记「${title}」？（正文移入 notes/.trash/）`)) return;
    await deleteNote(id);
    if (openId === id) openId = null;
  }
</script>

<div class="notes">
  {#if openId}
    <button class="back" onclick={() => (openId = null)}>← 返回笔记列表</button>
    <div class="editor-wrap">
      <NoteEditor noteId={openId} />
    </div>
  {:else}
    <div class="add">
      <input
        type="text"
        placeholder="新建笔记标题…"
        bind:value={newTitle}
        onkeydown={(e) => e.key === "Enter" && add()}
      />
      <button class="primary" onclick={add} disabled={!newTitle.trim()}>新建笔记</button>
    </div>
    {#if notesState.list.length === 0}
      <p class="empty">还没有笔记。把整理好的备忘沉淀成一篇篇主题笔记。</p>
    {/if}
    <ul class="list">
      {#each notesState.list as n (n.id)}
        <li class="note-row">
          <button class="open" onclick={() => (openId = n.id)}>
            <span class="ntitle">{n.title}</span>
            <span class="nmeta">{new Date(n.updatedAt).toLocaleDateString("zh-CN")}</span>
          </button>
          <button class="mini ren" onclick={() => {
            const t = window.prompt("重命名笔记", n.title);
            if (t && t.trim()) void renameNote(n.id, t.trim()).catch((e) => alert(`重命名失败：${e}`));
          }}>改名</button>
          <button class="mini del" onclick={() => remove(n.id, n.title)}>删除</button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .notes { display: flex; flex-direction: column; height: 100%; flex: 1; min-height: 0; }
  .add { display: flex; gap: 8px; padding: 14px 18px 6px; }
  .add input { flex: 1; }
  .empty { color: var(--color-text-dim); text-align: center; margin-top: 10vh; }
  .list { list-style: none; margin: 0; padding: 8px 18px; overflow-y: auto; }
  .note-row { display: flex; align-items: center; gap: 6px; padding: 6px 0; border-bottom: 1px solid var(--color-border); }
  .open { flex: 1; display: flex; justify-content: space-between; align-items: center; background: transparent; border: none; text-align: left; padding: 4px 6px; }
  .open:hover .ntitle { color: var(--color-primary); }
  .ntitle { font-size: 14px; }
  .nmeta { font-size: 11px; color: var(--color-text-dim); }
  .mini { font-size: 12px; padding: 2px 8px; }
  .del { color: #dc2626; }
  .ren { color: var(--color-text-dim); }
  .back { align-self: flex-start; margin: 10px 18px 0; }
  .editor-wrap { flex: 1; min-height: 0; }
</style>
