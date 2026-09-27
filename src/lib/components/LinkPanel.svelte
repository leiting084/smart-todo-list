<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ItemType, type Link } from "../api";
  import { ui } from "../stores.svelte";

  let { itemType, itemId }: { itemType: ItemType; itemId: string } = $props();

  type Row = {
    link: Link;
    /** 方向：out=本实体是 from；in=本实体是 to */
    dir: "out" | "in";
    otherType: ItemType;
    otherId: string;
    title: string | null;
  };
  let rows = $state<Row[]>([]);
  let loading = $state(true);

  const REL_LABEL: Record<string, string> = {
    converted_to: "转化 →",
    derived_from: "来源 ←",
    backlog: "生成待办 →",
    reference: "引用",
  };

  onMount(() => void reload());
  $effect(() => {
    void itemType;
    void itemId;
    void reload();
  });

  async function titleOf(type: ItemType, id: string): Promise<string | null> {
    try {
      if (type === "todo") return (await api.todoGet(id))?.title ?? null;
      if (type === "memo") {
        const m = (await api.memoList(true)).find((x) => x.id === id);
        return m?.title ?? null;
      }
      const n = (await api.noteList(true)).find((x) => x.id === id);
      return n?.title ?? null;
    } catch {
      return null;
    }
  }

  async function reload() {
    loading = true;
    try {
      const links = await api.linksForEntity(itemType, itemId);
      const next: Row[] = [];
      for (const l of links) {
        const dir = l.fromType === itemType && l.fromId === itemId ? "out" : "in";
        const otherType = dir === "out" ? l.toType : l.fromType;
        const otherId = dir === "out" ? l.toId : l.fromId;
        const title = await titleOf(otherType, otherId);
        next.push({ link: l, dir, otherType, otherId, title });
      }
      rows = next;
    } finally {
      loading = false;
    }
  }

  /** 点击跳转：todo 设为详情选中并切视图；memo/note 切视图（列表内自行查找） */
  function jump(type: ItemType, id: string) {
    if (type === "todo") {
      ui.activeTag = null;
      ui.activeTodoId = id;
      ui.overdueMode = false;
      ui.view = "today";
    } else if (type === "memo") {
      ui.activeTag = null;
      ui.view = "memos";
    } else {
      ui.activeTag = null;
      ui.view = "notes";
    }
  }

  const typeLabel: Record<string, string> = { todo: "待办", memo: "备忘", note: "笔记" };
</script>

<div class="linkpanel">
  <div class="head">
    <span class="t">关联链接</span>
    <button class="mini" onclick={() => void reload()} title="刷新">↻</button>
  </div>
  {#if loading}
    <p class="dim">加载中…</p>
  {:else if rows.length === 0}
    <p class="dim">暂无关联链接。</p>
  {:else}
    <ul>
      {#each rows as r (r.link.id)}
        <li class:stale={r.title === null}>
          <span class="rel">{REL_LABEL[r.link.relation] ?? r.link.relation}</span>
          <button class="jump" onclick={() => jump(r.otherType, r.otherId)}>
            [{typeLabel[r.otherType] ?? r.otherType}]
            {r.title ?? "（已删除，点击无效）"}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .linkpanel { margin-top: 4px; }
  .head { display: flex; align-items: center; gap: 6px; }
  .t { font-size: 12px; font-weight: 600; color: var(--color-text-dim); flex: 1; }
  .mini { font-size: 11px; padding: 0 6px; }
  .dim { color: var(--color-text-dim); font-size: 12px; }
  ul { list-style: none; margin: 4px 0 0; padding: 0; display: flex; flex-direction: column; gap: 3px; }
  li { display: flex; align-items: center; gap: 6px; font-size: 12px; min-width: 0; }
  .rel { color: var(--color-text-dim); white-space: nowrap; font-size: 11px; }
  .jump { text-align: left; border: none; background: transparent; padding: 1px 2px; font-size: 12px; color: var(--color-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  li.stale .jump { color: #dc2626; text-decoration: line-through; }
</style>
