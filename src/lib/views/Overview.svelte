<script lang="ts">
  // T3.6 月总览（主窗"总览"视图）：月历栅格 + 每日待办数/颜色点 + 点击定位当日 + 批量完成
  import { onMount } from "svelte";
  import { api, type DayStat, type MonthStats } from "../api";
  import { dayMark } from "../holidays";
  import { ui } from "../stores.svelte";

  let year = $state(0);
  let month = $state(0); // 1-12
  let stats = $state<MonthStats | null>(null);
  let todayYmd = $state(0);

  const WEEK_HEAD = ["日", "一", "二", "三", "四", "五", "六"];
  const YEARS = Array.from({ length: 2030 - 2024 + 1 }, (_, i) => 2024 + i);
  const MONTHS = Array.from({ length: 12 }, (_, i) => i + 1);
  const COLOR_HEX: Record<string, string> = {
    red: "#ef4444", orange: "#f97316", yellow: "#eab308", green: "#22c55e",
    blue: "#3b82f6", purple: "#a855f7", gray: "#9ca3af",
  };
  let exportMsg = $state("");

  onMount(async () => {
    const now = new Date();
    year = now.getFullYear();
    month = now.getMonth() + 1;
    todayYmd = await api.todayDate();
    await load();
  });

  async function load() {
    stats = await api.overviewMonth(year, month);
  }
  function prevMonth() {
    if (month === 1) { month = 12; year -= 1; } else { month -= 1; }
    void load();
  }
  function nextMonth() {
    if (month === 12) { month = 1; year += 1; } else { month += 1; }
    void load();
  }
  function gotoToday() {
    const now = new Date();
    year = now.getFullYear();
    month = now.getMonth() + 1;
    void load();
  }
  /** 年/月下拉跳转（select 用 onchange，checkbox 才有 onchange 委托问题） */
  function changeYear(e: Event) {
    year = Number((e.currentTarget as HTMLSelectElement).value);
    void load();
  }
  function changeMonth(e: Event) {
    month = Number((e.currentTarget as HTMLSelectElement).value);
    void load();
  }
  /** 批量取消完成某日全部已完成 */
  async function batchUncomplete(d: DayStat) {
    if (!window.confirm(`取消完成该日 ${d.done} 条已完成待办？`)) return;
    try {
      await api.overviewBatchUncomplete(d.date);
      await load();
    } catch (e) {
      alert(`操作失败：${e}`);
    }
  }
  /** 导出当月 MD（1 号 ~ 最后一天，含已完成），保存对话框与设置页导出同款 */
  async function exportMonthMd() {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const lastDay = new Date(year, month, 0).getDate();
      const dateFrom = year * 10000 + month * 100 + 1;
      const dateTo = year * 10000 + month * 100 + lastDay;
      const path = await save({
        filters: [{ name: "Markdown", extensions: ["md"] }],
        defaultPath: `待办导出-${year}${String(month).padStart(2, "0")}.md`,
      });
      if (!path) return;
      const [, n] = await api.exportMarkdown(String(path), {
        dateFrom,
        dateTo,
        includeDone: true,
      });
      exportMsg = `已导出 ${n} 条到 ${path}`;
    } catch (e) {
      alert(`导出失败：${e}`);
    }
  }
  function inCurrentMonth(d: DayStat) {
    return Math.floor(d.date / 100) === year * 100 + month;
  }
  /** F16：节假日 / 调休上班标记（数据见 lib/holidays.ts） */
  function mark(d: DayStat) {
    return dayMark(d.date);
  }

  function cellTitle(d: DayStat) {
    const m = dayMark(d.date);
    const parts: string[] = [];
    if (m.holiday) parts.push(m.name ?? "法定假日");
    if (m.workday) parts.push("调休上班");
    if (d.total > 0) parts.push(`${d.done}/${d.total} 完成`);
    return parts.join(" · ");
  }

  function cellLabel(d: DayStat) {
    return d.date % 100;
  }
  /** 点击格子：定位当日 → 今日视图加载该日期 */
  function jumpToDay(d: DayStat) {
    ui.pendingDate = d.date;
    ui.activeTag = null;
    // 不复位 overdueMode 的话，Today.refresh 会优先判逾期态、忽略 pendingDate
    ui.overdueMode = false;
    ui.view = "today";
  }
  async function batchComplete(d: DayStat) {
    if (!window.confirm(`已完成该日还有 ${d.done} 条，剩余 ${d.total - d.done} 条未完成。批量完成这一天的全部未完成？`)) return;
    await api.overviewBatchComplete(d.date);
    await load();
  }
</script>

<div class="overview">
  <header>
    <button onclick={prevMonth}>‹</button>
    <select class="ym-sel" value={year} onchange={changeYear} title="跳转年份">
      {#each YEARS as y (y)}
        <option value={y}>{y} 年</option>
      {/each}
    </select>
    <select class="ym-sel" value={month} onchange={changeMonth} title="跳转月份">
      {#each MONTHS as m (m)}
        <option value={m}>{m} 月</option>
      {/each}
    </select>
    <button onclick={nextMonth}>›</button>
    <button class="mini today-btn" onclick={gotoToday}>回到今天</button>
    <button class="mini" onclick={exportMonthMd} title="导出当月 1 号至最后一天（含已完成）为 Markdown">导出当月 MD</button>
    {#if exportMsg}<span class="export-msg">{exportMsg}</span>{/if}
    <span class="sum" title="当月完成 / 当月全部 · 收集箱">
      完成 {stats?.monthDone ?? 0}/{stats?.monthTotal ?? 0} · 收集箱 {stats?.inboxCount ?? 0}
    </span>
  </header>

  <div class="grid">
    {#each WEEK_HEAD as w}
      <div class="head">{w}</div>
    {/each}
    {#each stats?.days ?? [] as d (d.date)}
      <!-- a11y 检查忽略：日历格 tabindex 供方向键聚焦 -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="cell"
        class:cur={inCurrentMonth(d)}
        class:today={d.date === todayYmd}
        class:has={d.total > 0}
        class:hol={mark(d).holiday}
        class:work={mark(d).workday}
        role={inCurrentMonth(d) ? "gridcell" : undefined}
        tabindex={d.date === todayYmd ? 0 : undefined}
        onclick={() => inCurrentMonth(d) && jumpToDay(d)}
        onkeydown={(e) => (e.key === "Enter" || e.key === " ") && inCurrentMonth(d) && jumpToDay(d)}
        title={cellTitle(d)}
      >
        <span class="day-num">{cellLabel(d)}</span>
        {#if mark(d).holiday}
          <span class="tag hol-tag">{mark(d).name ?? "休"}</span>
        {:else if mark(d).workday}
          <span class="tag work-tag">班</span>
        {/if}
        {#if d.total > 0}
          <div class="info">
            <div class="dots">
              {#each d.colors.slice(0, 3) as c (c)}
                <span class="dot" style={`background:${COLOR_HEX[c] ?? "#888"}`}></span>
              {/each}
            </div>
            <span class="cnt">{d.done}/{d.total}</span>
            {#if d.done > 0}
              <button
                class="mini-u"
                onclick={(e) => { e.stopPropagation(); void batchUncomplete(d); }}
                title="批量取消完成当日已完成"
              >↺</button>
            {/if}
            <button
              class="mini-c"
              onclick={(e) => { e.stopPropagation(); void batchComplete(d); }}
              title="批量完成当日未完成（单事务）"
            >✓</button>
          </div>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .overview { padding: 12px 16px; overflow-y: auto; flex: 1; display: flex; flex-direction: column; gap: 10px; }
  header { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  header button { font-size: 14px; padding: 2px 10px; }
  .ym-sel { font-size: 13px; padding: 2px 4px; font-weight: 600; }
  .export-msg { font-size: 12px; color: var(--color-primary); }
  .mini { font-size: 12px; padding: 2px 8px; }
  .sum { margin-left: auto; font-size: 12px; color: var(--color-text-dim); }
  .grid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 3px; }
  .head { text-align: center; font-size: 11px; color: var(--color-text-dim); padding: 2px; }
  .cell {
    border: 1px solid var(--color-border);
    border-radius: 6px;
    min-height: 64px;
    padding: 3px 5px;
    font-size: 12px;
    color: var(--color-text-dim);
    cursor: default;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .cell.cur { background: var(--color-surface); color: var(--color-text); cursor: pointer; }
  .cell.cur:hover { border-color: var(--color-primary); }
  .cell.today .day-num { color: var(--color-primary); font-weight: 700; }
  .cell.has { border-color: var(--color-primary-soft); }
  /* F16 节假日：放假日淡红底 + 节日名；调休上班日灰底 + "班" */
  .cell.hol { background: rgba(217, 75, 75, 0.10); }
  .cell.work { background: rgba(136, 135, 128, 0.12); }
  .day-num { text-align: right; font-size: 11px; }
  .tag { text-align: right; font-size: 10px; line-height: 1.1; }
  .hol-tag { color: #e0685f; }
  .work-tag { color: var(--color-text-dim); }
  .info { display: flex; align-items: center; gap: 4px; }
  .dots { display: flex; gap: 2px; }
  .dot { width: 6px; height: 6px; border-radius: 50%; }
  .cnt { font-size: 11px; }
  .mini-c { margin-left: auto; font-size: 11px; padding: 0 4px; border-color: transparent; background: transparent; color: var(--color-primary); }
  .mini-c:hover { border-color: var(--color-primary); }
  .mini-u { font-size: 11px; padding: 0 4px; border-color: transparent; background: transparent; color: var(--color-text-dim); }
  .mini-u:hover { color: var(--color-primary); border-color: var(--color-primary); }
</style>