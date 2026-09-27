<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import DOMPurify from "dompurify";
  import { marked } from "marked";
  import { api, type ItemType, type Note } from "../api";
  import { notesState } from "../notes.svelte";
  import { ui } from "../stores.svelte";

  let { noteId }: { noteId: string } = $props();

  let note = $state<Note | null>(null);
  let content = $state("");
  let saveState = $state<"saved" | "dirty" | "saving" | "error">("saved");
  let saveError = $state("");
  let previewEl = $state<HTMLDivElement | null>(null);

  // ---- F6/T2.4 生成待办 ----
  let selectedText = $state("");
  let todoBarDate = $state("");
  let showTodoBar = $state(false);

  // 预览：wiki-link 先替换为内部锚链接，marked 渲染，DOMPurify 净化（SPEC §9）
  const previewHtml = $derived.by(() => {
    const md = content.replace(
      /\[\[(todo|memo|note):([^\]\s]+)\]\]/g,
      (_m, t: string, id: string) => `[${t}:${id}](#wiki-${t}-${id})`,
    );
    const raw = marked.parse(md, { async: false }) as string;
    return DOMPurify.sanitize(raw, { USE_PROFILES: { html: true } });
  });

  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(async () => {
    const { note: n, content: c } = await api.noteGetContent(noteId);
    note = n;
    content = c;
  });

  // 切换笔记前尽力保存
  $effect(() => {
    const id = noteId;
    return () => {
      if (id !== noteId && saveState === "dirty") {
        void api.noteSaveContent(id, content).catch(() => {});
      }
    };
  });

  async function save() {
    if (!note) return;
    clearTimeout(debounceTimer);
    saveState = "saving";
    try {
      const n = await api.noteSaveContent(note.id, content);
      note = n;
      notesState.list = notesState.list.map((x) => (x.id === n.id ? n : x));
      saveState = "saved";
    } catch (e) {
      saveState = "error";
      saveError = String(e);
    }
  }

  function onInput() {
    saveState = "dirty";
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => void save(), 500);
  }

  function onKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === "s") {
      e.preventDefault();
      void save();
    }
  }

  onDestroy(() => {
    clearTimeout(debounceTimer);
    if (saveState === "dirty" && note) {
      void api.noteSaveContent(note.id, content).catch(() => {});
    }
  });

  const SAVE_LABEL: Record<string, string> = {
    saved: "已保存",
    dirty: "未保存…",
    saving: "保存中…",
    error: "保存失败",
  };

  // ---- wiki-link 后处理：标题替换 + 失效态 + 点击跳转（F7） ----
  async function titleOf(type: ItemType, id: string): Promise<string | null> {
    try {
      if (type === "todo") return (await api.todoGet(id))?.title ?? null;
      if (type === "memo") {
        return (await api.memoList(true)).find((x) => x.id === id)?.title ?? null;
      }
      return (await api.noteList(true)).find((x) => x.id === id)?.title ?? null;
    } catch {
      return null;
    }
  }

  $effect(() => {
    const html = previewHtml;
    if (!html || !previewEl) return;
    void tick().then(async () => {
      if (!previewEl) return;
      const anchors = previewEl.querySelectorAll<HTMLAnchorElement>('a[href^="#wiki-"]');
      for (const a of anchors) {
        const m = a.getAttribute("href")?.match(/^#wiki-(todo|memo|note)-(.+)$/);
        if (!m) continue;
        const type = m[1] as ItemType;
        const id = m[2];
        const title = await titleOf(type, id);
        a.textContent = title ?? `${type}:${id}（已失效）`;
        if (!title) a.classList.add("stale");
        a.onclick = (e) => {
          e.preventDefault();
          if (!title) return;
          ui.activeTag = null;
          if (type === "todo") {
            ui.activeTodoId = id;
            ui.overdueMode = false;
            ui.view = "today";
          } else if (type === "memo") ui.view = "memos";
          else ui.view = "notes";
        };
      }
    });
  });

  // ---- 生成待办（F6） ----
  let textareaEl = $state<HTMLTextAreaElement | null>(null);
  function captureSelection() {
    if (!textareaEl) return;
    const sel = textareaEl.value.slice(textareaEl.selectionStart, textareaEl.selectionEnd).trim();
    if (sel) {
      selectedText = sel;
      showTodoBar = true;
    }
  }
  async function confirmCreateTodo() {
    if (!note || !selectedText) return;
    try {
      const date = todoBarDate ? Number(todoBarDate.replaceAll("-", "")) : null;
      await api.noteCreateTodo(note.id, selectedText, date);
      showTodoBar = false;
      selectedText = "";
      todoBarDate = "";
      alert("已生成待办（含反链），可在「今天」或收集箱查看。");
    } catch (e) {
      alert(`生成待办失败：${e}`);
    }
  }
</script>

<div class="editor">
  <div class="toolbar">
    <span class="title">{note?.title ?? "…"}</span>
    <button class="mini" onclick={() => (showTodoBar = !showTodoBar)} title="选中文字后点击，生成待办（带反链）">
      生成待办
    </button>
    <span class="save-state" class:err={saveState === "error"}>{SAVE_LABEL[saveState]}</span>
  </div>

  {#if showTodoBar}
    <div class="todo-bar">
      <span class="sel-text">「{selectedText.slice(0, 40) || "（先在正文中选中文字）"}」</span>
      <input type="date" bind:value={todoBarDate} title="待办日期（可空=收集箱）" />
      <button class="primary" onclick={confirmCreateTodo} disabled={!selectedText}>生成</button>
      <button onclick={() => (showTodoBar = false)}>取消</button>
    </div>
  {/if}

  {#if saveState === "error"}
    <p class="err">{saveError}</p>
  {/if}
  <div class="split">
    <textarea
      class="src"
      bind:value={content}
      bind:this={textareaEl}
      oninput={onInput}
      onkeydown={onKeydown}
      onmouseup={captureSelection}
      placeholder="Markdown…（用 [[todo:ID]] 引用待办）"
      spellcheck="false"
    ></textarea>
    <div class="preview" bind:this={previewEl}>{@html previewHtml}</div>
  </div>
</div>

<style>
  .editor { display: flex; flex-direction: column; height: 100%; }
  .toolbar { display: flex; align-items: center; gap: 10px; padding: 10px 16px; border-bottom: 1px solid var(--color-border); background: var(--color-surface); }
  .title { font-weight: 600; flex: 1; }
  .mini { font-size: 12px; padding: 2px 8px; }
  .save-state { font-size: 12px; color: var(--color-text-dim); }
  .save-state.err { color: #dc2626; }
  .err { color: #dc2626; padding: 4px 16px; margin: 0; font-size: 12px; }
  .todo-bar { display: flex; align-items: center; gap: 8px; padding: 8px 16px; background: var(--color-primary-soft); border-bottom: 1px solid var(--color-primary); }
  .sel-text { flex: 1; font-size: 12px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .split { flex: 1; min-height: 0; display: grid; grid-template-columns: 1fr 1fr; }
  .src { border: none; resize: none; padding: 14px 16px; font-family: Consolas, "Microsoft YaHei", monospace; font-size: 13px; line-height: 1.7; background: var(--color-bg); color: var(--color-text); outline: none; }
  .preview { border-left: 1px solid var(--color-border); padding: 14px 18px; overflow-y: auto; background: var(--color-surface); font-size: 14px; line-height: 1.7; }
  .preview :global(h1), .preview :global(h2), .preview :global(h3) { border-bottom: 1px solid var(--color-border); padding-bottom: 4px; }
  .preview :global(code) { background: var(--color-bg); border-radius: 4px; padding: 1px 5px; font-family: Consolas, monospace; font-size: 12px; }
  .preview :global(pre) { background: var(--color-bg); padding: 10px; border-radius: 6px; overflow-x: auto; }
  .preview :global(pre code) { background: transparent; padding: 0; }
  .preview :global(blockquote) { border-left: 3px solid var(--color-primary); margin: 8px 0; padding: 2px 12px; color: var(--color-text-dim); }
  .preview :global(a[href^="#wiki-"]) { color: var(--color-primary); }
  .preview :global(a.stale) { color: #dc2626; text-decoration: line-through; }
</style>
