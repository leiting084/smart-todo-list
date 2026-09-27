<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    type Goal,
    type GoalLink,
    type GoalStats,
    type ItemType,
  } from "../api";
  import { archiveGoal, createGoal, goalsState, loadGoals, patchGoal, setGoalStatus } from "../goals.svelte";

  let newTitle = $state("");
  let selectedId = $state<string | null>(null);
  let links = $state<GoalLink[]>([]);
  let stats = $state<GoalStats | null>(null);
  // 手动进度滑块草稿：拖动中未提交的值（null = 未在拖动）
  let progressDraft = $state<number | null>(null);
  // 目标日期编辑草稿（YYYY-MM-DD，空串=未设）
  let dateDraft = $state("");
  const selected = $derived(goalsState.list.find((g) => g.id === selectedId) ?? null);
  // 关联待办完成率（F32：目标主进度=该完成率；无关联待办时回退手动进度）
  const linkPct = $derived(Math.round((stats?.progress ?? 0) * 100));
  const hasLinks = $derived((stats?.todoCount ?? 0) > 0);
  const displayPct = $derived(hasLinks ? linkPct : (progressDraft ?? selected?.progress ?? 0));
  /** 列表卡片进度：有关联待办用完成率，否则回退手动进度 */
  function goalPct(g: Goal): number {
    return g.todoCount > 0 ? Math.round(g.linkProgress * 100) : g.progress;
  }

  /** 毫秒时间戳 → YYYY-MM-DD（本地时区显示） */
  function fmtDate(ts: number | null | undefined): string {
    if (!ts) return "";
    const d = new Date(ts);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  }
  /** YYYY-MM-DD → 本地零点毫秒时间戳 */
  function parseDate(s: string): number | null {
    if (!s) return null;
    const ts = new Date(`${s}T00:00:00`).getTime();
    return Number.isNaN(ts) ? null : ts;
  }

  onMount(() => loadGoals());

  async function add() {
    const title = newTitle.trim();
    if (!title) return;
    newTitle = "";
    try {
      await createGoal(title);
    } catch (e) {
      alert(`创建失败：${e}`);
    }
  }

  async function open(g: Goal) {
    selectedId = g.id;
    progressDraft = null;
    dateDraft = fmtDate(g.targetDate);
    await refreshDetail();
  }
  async function refreshDetail() {
    if (!selectedId) return;
    [links, stats] = await Promise.all([
      api.goalGetLinks(selectedId),
      api.goalGetStats(selectedId),
    ]);
  }
  async function remove(g: Goal) {
    if (!window.confirm(`归档目标「${g.title}」？`)) return;
    await archiveGoal(g.id);
    selectedId = null;
  }
  async function toggleDone(g: Goal) {
    await setGoalStatus(g.id, g.status === "done" ? "active" : "done");
  }

  /** 提交手动进度（0-100，与关联待办无关） */
  async function commitProgress() {
    if (!selected || progressDraft === null) return;
    const v = progressDraft;
    progressDraft = null;
    try {
      await patchGoal(selected.id, { progress: v });
    } catch (e) {
      alert(`更新进度失败：${e}`);
    }
  }
  /** 提交目标日期；清空 = null */
  async function commitDate() {
    if (!selected) return;
    const ts = parseDate(dateDraft);
    try {
      await patchGoal(selected.id, { targetDate: ts });
    } catch (e) {
      alert(`更新目标日期失败：${e}`);
    }
  }

  const typeLabel: Record<string, string> = { todo: "待办", memo: "备忘", note: "笔记" };
  function linkText(l: GoalLink) {
    return l.title === null ? `（已删除的${typeLabel[l.itemType] ?? l.itemType}）` : l.title;
  }

  // 关联管理：勾选待办列表 + 已关联集合
  let pickerTodos = $state<{ id: string; title: string }[]>([]);
  let showPicker = $state(false);
  async function openPicker() {
    if (!selectedId) return;
    const todos = await api.todoList({});
    pickerTodos = todos.map((t) => ({ id: t.id, title: t.title }));
    showPicker = true;
  }
  /** join=true 加入关联；false 移除 */
  async function toggleLink(itemId: string, itemType: ItemType, join: boolean) {
    if (!selectedId) return;
    const current = links.map((l) => ({ itemType: l.itemType, itemId: l.itemId }));
    const items = join
      ? [...current, { itemType, itemId }]
      : current.filter((l) => !(l.itemType === itemType && l.itemId === itemId));
    try {
      links = await api.goalSetLinks(selectedId, items);
      await refreshDetail();
      // 关联变化会影响列表卡片的完成率，重新拉取列表让卡片即时刷新
      await loadGoals();
    } catch (e) {
      alert(`关联失败：${e}`);
    }
  }
</script>

{#if selected}
  <div class="detail">
    <button class="back" onclick={() => (selectedId = null)}>← 返回目标列表</button>
    <div class="head">
      <h2>{selected.title}</h2>
      {#if selected.category}<span class="cat">{selected.category}</span>{/if}
      <button onclick={() => toggleDone(selected)}>
        {selected.status === "done" ? "✓ 已达成（点击恢复）" : "标记达成"}
      </button>
      <button class="del" onclick={() => remove(selected)}>归档</button>
    </div>
    {#if selected.description}<p class="desc">{selected.description}</p>{/if}

    <div class="progress">
      <div class="prog-row">
        <div class="bar"><div class="fill" style={`width:${displayPct}%`}></div></div>
        <span class="pct-num">{displayPct}%</span>
      </div>
      {#if hasLinks}
        <div class="prog-src">进度 = 关联待办完成率（{stats?.completedCount ?? 0}/{stats?.todoCount ?? 0}），随待办勾选自动更新</div>
      {:else}
        <input
          type="range"
          class="slider"
          min="0"
          max="100"
          step="5"
          value={progressDraft ?? selected.progress}
          oninput={(e) => (progressDraft = Number((e.target as HTMLInputElement).value))}
          onchange={commitProgress}
          onkeydown={(e) => e.key === "Enter" && commitProgress()}
          title="尚无关联待办，可手动设定进度；关联待办后将自动按完成率计算"
        />
        <div class="prog-src">尚无关联待办，当前为手动进度；关联待办后将自动按完成率计算</div>
      {/if}
      <div class="prog-meta">
        <label class="date-edit">
          目标日期
          <input type="date" bind:value={dateDraft} onchange={commitDate} />
          {#if !dateDraft}
            <button class="mini" onclick={commitDate} title="清除目标日期">清除</button>
          {/if}
        </label>
      </div>
      {#if stats?.memoCount || stats?.noteCount}
        <div class="link-ref">关联备忘 {stats?.memoCount ?? 0} · 笔记 {stats?.noteCount ?? 0}</div>
      {/if}
    </div>

    <div class="links-head">
      <h3>关联（{links.length}）</h3>
      <button class="mini" onclick={openPicker}>关联待办…</button>
    </div>
    {#if showPicker}
      <div class="picker">
        {#if pickerTodos.length === 0}
          <span class="dim">还没有待办。</span>
        {/if}
        {#each pickerTodos as t (t.id)}
          <label class="pick-row">
            <input
              type="checkbox"
              checked={links.some((l) => l.itemType === "todo" && l.itemId === t.id)}
              onchange={(e) =>
                toggleLink(t.id, "todo", (e.target as HTMLInputElement).checked)}
            />
            {t.title}
          </label>
        {/each}
        <button class="mini" onclick={() => (showPicker = false)}>收起</button>
      </div>
    {/if}
    <ul class="links">
      {#each links as l (l.itemType + l.itemId)}
        <li class:stale={l.title === null}>
          <span class="type">[{typeLabel[l.itemType] ?? l.itemType}]</span>
          {#if l.itemType === "todo" && l.title !== null}
            <input
              type="checkbox"
              checked={l.completed}
              onchange={() => toggleLink(l.itemId, "todo", false)}
              title="取消勾选=从目标移除该待办"
            />
          {/if}
          <span>{linkText(l)}</span>
          <button class="unlink" onclick={() => toggleLink(l.itemId, l.itemType, true)} title="移除关联">
            ✕
          </button>
        </li>
      {/each}
      {#if links.length === 0}
        <li class="dim">还没有关联。待办/备忘/笔记都可以挂到这个目标上。</li>
      {/if}
    </ul>
  </div>
{:else}
  <div class="list">
    <div class="add">
      <input
        type="text"
        placeholder="新建长期目标…（如：提升技术能力）"
        bind:value={newTitle}
        onkeydown={(e) => e.key === "Enter" && add()}
      />
      <button class="primary" onclick={add} disabled={!newTitle.trim()}>新建目标</button>
    </div>
    {#if goalsState.list.length === 0}
      <p class="empty">还没有长期目标。目标可关联待办/备忘/笔记；进度按关联待办完成率自动计算（无关联时可手动设定）。</p>
    {/if}
    <div class="cards">
      {#each goalsState.list as g (g.id)}
        <button class="card" onclick={() => open(g)}>
          <span class="card-top">
            <span class="name">{g.title}</span>
            {#if g.status === "done"}<span class="badge">已达成</span>{/if}
          </span>
          {#if g.description}<span class="card-desc">{g.description}</span>{/if}
          <span class="card-prog">
            <span class="mini-bar"><span class="mini-fill" style={`width:${goalPct(g)}%`}></span></span>
            <span class="mini-pct">{goalPct(g)}%</span>
            {#if g.targetDate}<span class="mini-date">→ {fmtDate(g.targetDate)}</span>{/if}
          </span>
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
  .card { text-align: left; padding: 12px; display: flex; flex-direction: column; gap: 6px; background: var(--color-surface); }
  .card-top { display: flex; align-items: center; gap: 8px; font-weight: 600; }
  .card-desc { font-size: 12px; color: var(--color-text-dim); }
  .badge { font-size: 11px; color: var(--color-primary); border: 1px solid var(--color-primary); border-radius: 8px; padding: 0 6px; }
  .back { margin-bottom: 10px; }
  .head { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .head h2 { margin: 0; font-size: 18px; flex: 1; }
  .cat { font-size: 12px; color: var(--color-text-dim); border: 1px solid var(--color-border); border-radius: 8px; padding: 0 8px; }
  .desc { color: var(--color-text-dim); font-size: 13px; }
  .progress { display: flex; flex-direction: column; gap: 6px; margin: 12px 0; }
  .prog-row { display: flex; align-items: center; gap: 10px; }
  .bar { flex: 1; height: 8px; background: var(--color-border); border-radius: 4px; overflow: hidden; }
  .fill { height: 100%; background: var(--color-primary); transition: width 0.1s; }
  .pct-num { font-size: 14px; font-weight: 600; color: var(--color-primary); min-width: 44px; text-align: right; }
  .slider { width: 100%; accent-color: var(--color-primary); }
  .prog-meta { display: flex; align-items: center; gap: 12px; }
  .date-edit { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--color-text-dim); }
  .link-ref { font-size: 12px; color: var(--color-text-dim); }
  .prog-src { font-size: 12px; color: var(--color-text-dim); }
  .card-prog { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--color-text-dim); }
  .mini-bar { flex: 1; height: 5px; background: var(--color-border); border-radius: 3px; overflow: hidden; }
  .mini-fill { display: block; height: 100%; background: var(--color-primary); }
  .mini-pct { min-width: 30px; text-align: right; }
  .mini-date { white-space: nowrap; }
  .links-head { display: flex; align-items: center; gap: 10px; margin: 12px 0 6px; }
  .links-head h3 { font-size: 13px; margin: 0; flex: 1; }
  .mini { font-size: 12px; padding: 2px 8px; }
  .picker { border: 1px dashed var(--color-border); border-radius: 8px; padding: 10px; margin-bottom: 8px; max-height: 200px; overflow-y: auto; }
  .pick-row { display: flex; align-items: center; gap: 6px; padding: 2px 0; font-size: 13px; cursor: pointer; }
  .links { list-style: none; margin: 0; padding: 0; }
  .links li { display: flex; align-items: center; gap: 8px; padding: 4px 0; }
  .links .type { font-size: 11px; color: var(--color-text-dim); }
  .links li.stale span:not(.type) { color: #dc2626; text-decoration: line-through; }
  .unlink { border: none; background: transparent; color: var(--color-text-dim); padding: 0 6px; }
  .unlink:hover { color: #dc2626; }
  .del { color: #dc2626; }
  .dim { color: var(--color-text-dim); }
</style>
