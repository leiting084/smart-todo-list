<script lang="ts">
  // T4.5 全文搜索：侧栏搜索框 → 三类结果（todos/memos/notes）+ 点击跳转
  import { api, type SearchResult } from "../api";
  import { ui } from "../stores.svelte";

  let q = $state("");
  let result = $state<SearchResult | null>(null);
  let debounce: ReturnType<typeof setTimeout> | undefined;
  let searching = $state(false);

  function onInput(value: string) {
    q = value;
    if (!value.trim()) { result = null; return; }
    clearTimeout(debounce);
    searching = true;
    debounce = setTimeout(async () => {
      try {
        result = await api.searchAll(value.trim());
      } catch (e) {
        result = null;
        console.error(e);
      } finally {
        searching = false;
      }
    }, 300);
  }

  function jumpTodo(id: string) {
    ui.activeTodoId = id;
    ui.activeTag = null;
    ui.overdueMode = false;
    ui.view = "today";
    q = "";
    result = null;
  }
  function jump(kind: "memos" | "notes") {
    ui.activeTag = null;
    ui.view = kind === "memos" ? "memos" : "notes";
    q = "";
    result = null;
  }
</script>

<div class="search">
  <input type="text" placeholder="搜索待办 / 备忘 / 笔记…" value={q}
    oninput={(e) => onInput((e.target as HTMLInputElement).value)}
    onkeydown={(e) => e.key === "Escape" && ((q = ""), (result = null))}
  />
  {#if q && !searching && result}
    {#if result.todos.length || result.memos.length || result.notes.length}
      <div class="results">
        {#if result.todos.length}
          <div class="group">
            <div class="g-title">待办（{result.todos.length}）</div>
            {#each result.todos.slice(0, 8) as t (t.id)}
              <button class="row" onclick={() => jumpTodo(t.id)} title={t.title}>
                <span class="dot" class:done={t.completed}></span>
                {t.title}
              </button>
            {/each}
          </div>
        {/if}
        {#if result.memos.length}
          <div class="group">
            <div class="g-title">备忘（{result.memos.length}）</div>
            {#each result.memos.slice(0, 5) as m (m.id)}
              <button class="row" onclick={() => jump("memos")} title={m.title}>{m.title}</button>
            {/each}
          </div>
        {/if}
        {#if result.notes.length}
          <div class="group">
            <div class="g-title">笔记（{result.notes.length}）</div>
            {#each result.notes.slice(0, 5) as n (n.id)}
              <button class="row" onclick={() => jump("notes")} title={n.title}>{n.title}</button>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <div class="results"><p class="none">无匹配结果</p></div>
    {/if}
  {/if}
</div>

<style>
  .search { padding: 8px 10px; border-top: 1px solid var(--color-border); position: relative; }
  .search input { width: 100%; font-size: 12px; }
  .results {
    position: absolute;
    left: 10px;
    right: 10px;
    bottom: calc(100% + 4px);
    max-height: 40vh;
    overflow-y: auto;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    box-shadow: var(--shadow);
    z-index: 30;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .group { display: flex; flex-direction: column; gap: 2px; }
  .g-title { font-size: 11px; color: var(--color-text-dim); }
  .row {
    text-align: left;
    background: transparent;
    border: none;
    font-size: 12px;
    padding: 3px 4px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row:hover { background: var(--color-hover); color: var(--color-primary); }
  .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: var(--color-primary); margin-right: 6px; }
  .dot.done { background: var(--color-border); }
  .none { font-size: 12px; color: var(--color-text-dim); margin: 2px 4px; }
</style>