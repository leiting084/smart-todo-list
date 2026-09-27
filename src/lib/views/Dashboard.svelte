<script lang="ts">
  import { onMount } from "svelte";
  import { api, type DashboardStats } from "../api";
  import AiReport from "../components/AiReport.svelte";
  import { ui } from "../stores.svelte";

  let stats = $state<DashboardStats | null>(null);
  let error = $state("");

  const COLOR_HEX: Record<string, string> = {
    red: "#ef4444", orange: "#f97316", yellow: "#eab308", green: "#22c55e",
    blue: "#3b82f6", purple: "#a855f7", gray: "#9ca3af",
  };

  onMount(async () => {
    try {
      stats = await api.dashboardGet();
    } catch (e) {
      error = String(e);
    }
  });

  const trendMax = $derived(Math.max(1, ...(stats?.weekTrend ?? []).map((p) => p.completed)));
  const weekdayLabel = (ymd: number) => {
    const d = new Date(Math.floor(ymd / 10000), Math.floor((ymd % 10000) / 100) - 1, ymd % 100);
    return ["日", "一", "二", "三", "四", "五", "六"][d.getDay()];
  };

  function goToday() {
    ui.activeTag = null;
    ui.overdueMode = false;
    ui.view = "today";
  }
  function goProject(id: string) {
    ui.activeTag = null;
    ui.view = "projects";
    void id;
  }
  function goOverdue() {
    ui.activeTag = null;
    // 交给今日视图在挂载时按逾期口径加载（避免与 Today.onMount 的今日刷新竞争）
    ui.overdueMode = true;
    ui.view = "today";
  }
</script>

<div class="dash">
  {#if error}
    <p class="err">{error}</p>
  {/if}

  {#if stats}
    {#if stats.todayCount === 0 && stats.overdueCount === 0 && stats.weekCompleted === 0 && stats.projectDistribution.length === 0 && stats.goalProgress.length === 0}
      <div class="empty">
        <p>欢迎使用 TodoList</p>
        <p class="hint">左侧「今天」添加第一条待办，仪表板会在这里呈现统计。</p>
        <button class="primary" onclick={goToday}>去添加待办</button>
      </div>
    {:else}
      <!-- 三个计数卡 -->
      <div class="cards">
        <button class="card stat" onclick={goToday}>
          <span class="num">{stats.todayCount}</span>
          <span class="label">今日待办</span>
        </button>
        <button class="card stat" class:alert={stats.overdueCount > 0} onclick={goOverdue}>
          <span class="num">{stats.overdueCount}</span>
          <span class="label">逾期</span>
        </button>
        <div class="card stat">
          <span class="num">{stats.weekCompleted}</span>
          <span class="label">本周完成</span>
        </div>
      </div>

      <div class="row2">
        <!-- 项目分布 -->
        <section class="panel">
          <h3>项目分布</h3>
          {#if stats.projectDistribution.length === 0}
            <p class="dim">暂无项目。</p>
          {:else}
            {#each stats.projectDistribution as p (p.projectId)}
              <button class="proj-row" onclick={() => goProject(p.projectId)} title="打开项目视图">
                <span class="dot" style={p.color ? `background:${COLOR_HEX[p.color] ?? "#9ca3af"}` : ""}></span>
                <span class="pname">{p.name}</span>
                <span class="pbar">
                  <span class="pfill" style={`width:${p.total === 0 ? 0 : Math.round((p.done / p.total) * 100)}%`}></span>
                </span>
                <span class="pcount">{p.done}/{p.total}</span>
              </button>
            {/each}
          {/if}
        </section>

        <!-- 目标进度 -->
        <section class="panel">
          <h3>目标进度</h3>
          {#if stats.goalProgress.length === 0}
            <p class="dim">暂无进行中的目标。</p>
          {:else}
            {#each stats.goalProgress as g (g.goalId)}
              <div class="goal-row">
                <span class="gname">{g.title}</span>
                <span class="gbar"><span class="gfill" style={`width:${Math.round(g.progress * 100)}%`}></span></span>
                <span class="gcount">{g.doneCount}/{g.todoCount}</span>
              </div>
            {/each}
          {/if}
        </section>
      </div>

      <!-- 近 7 天完成趋势（纯 CSS 柱条） -->
      <section class="panel">
        <h3>近 7 天完成趋势</h3>
        <div class="trend">
          {#each stats.weekTrend as p, i (p.date)}
            <div class="tcol">
              <div class="tbar-wrap">
                <div class="tbar" style={`height:${Math.round((p.completed / trendMax) * 100)}%`}></div>
              </div>
              <span class="tnum">{p.completed}</span>
              <span class="tlabel">{i === 6 ? "今天" : weekdayLabel(p.date)}</span>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  {:else}
    <p class="dim">加载中…</p>
  {/if}

  <AiReport />
</div>

<style>
  .dash { padding: 14px 18px; overflow-y: auto; flex: 1; display: flex; flex-direction: column; gap: 14px; }
  .err { color: #dc2626; }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 8px; margin-top: 15vh; color: var(--color-text); }
  .empty p { margin: 0; font-size: 16px; font-weight: 600; }
  .hint { font-size: 13px; color: var(--color-text-dim); font-weight: 400; }
  .dim { color: var(--color-text-dim); font-size: 13px; }

  .cards { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }
  .card.stat { display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 14px; background: var(--color-surface); }
  .num { font-size: 26px; font-weight: 700; color: var(--color-primary); }
  .label { font-size: 12px; color: var(--color-text-dim); }
  .card.stat.alert .num { color: #dc2626; }

  .row2 { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; }
  .panel { background: var(--color-surface); border: 1px solid var(--color-border); border-radius: 8px; padding: 12px 14px; }
  .panel h3 { margin: 0 0 10px; font-size: 13px; color: var(--color-text-dim); }

  .proj-row { display: flex; align-items: center; gap: 8px; width: 100%; text-align: left; padding: 4px 0; background: transparent; border: none; }
  .proj-row:hover .pname { color: var(--color-primary); }
  .dot { width: 10px; height: 10px; border-radius: 50%; flex-shrink: 0; }
  .pname { width: 90px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .pbar { flex: 1; height: 6px; background: var(--color-border); border-radius: 3px; overflow: hidden; }
  .pfill { display: block; height: 100%; background: var(--color-primary); }
  .pcount { font-size: 11px; color: var(--color-text-dim); white-space: nowrap; }

  .goal-row { display: flex; align-items: center; gap: 8px; padding: 4px 0; }
  .gname { width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .gbar { flex: 1; height: 6px; background: var(--color-border); border-radius: 3px; overflow: hidden; }
  .gfill { display: block; height: 100%; background: var(--color-primary); }
  .gcount { font-size: 11px; color: var(--color-text-dim); white-space: nowrap; }

  .trend { display: flex; align-items: flex-end; gap: 14px; height: 120px; padding: 0 6px; }
  .tcol { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: flex-end; height: 100%; gap: 3px; }
  .tbar-wrap { flex: 1; width: 100%; max-width: 42px; display: flex; align-items: flex-end; background: var(--color-bg); border-radius: 4px; overflow: hidden; }
  .tbar { width: 100%; background: var(--color-primary); border-radius: 4px 4px 0 0; min-height: 0; }
  .tnum { font-size: 11px; color: var(--color-text-dim); }
  .tlabel { font-size: 11px; color: var(--color-text-dim); }
</style>
