<script lang="ts">
  /**
   * 日期栏（F16 补：跨日期拖拽改期）。
   *
   * External 支持把待办直接拖到日期栏改期，此前我们只能走详情面板的日期选择器。
   * 这里提供同屏的放置目标：把 TodoList 里的待办拖到任意一格即改期到该日。
   *
   * 拖拽由 TodoList 的指针（pointer-events）引擎驱动：它按每格的 `[data-ymd]` 命中，
   * 悬停时写共享 store `drag.hoverDate` 高亮该格，松手时直接改期（不再走 HTML5 原生拖拽）。
   */
  import { dayMark } from "../holidays";
  import { drag } from "../drag.svelte";

  let {
    start,
    count = 7,
  }: { start: number; count?: number; onMoved: () => void } = $props();

  const WEEK = ["日", "一", "二", "三", "四", "五", "六"];

  function ymdAddDays(ymd: number, n: number): number {
    const y = Math.floor(ymd / 10000);
    const m = Math.floor((ymd % 10000) / 100);
    const d = ymd % 100;
    const dt = new Date(y, m - 1, d + n);
    return dt.getFullYear() * 10000 + (dt.getMonth() + 1) * 100 + dt.getDate();
  }

  const days = $derived(Array.from({ length: count }, (_, i) => ymdAddDays(start, i)));

  function info(ymd: number) {
    const y = Math.floor(ymd / 10000);
    const m = Math.floor((ymd % 10000) / 100);
    const d = ymd % 100;
    const wd = new Date(y, m - 1, d).getDay();
    return { d, wd, mark: dayMark(ymd), isWeekend: wd === 0 || wd === 6 };
  }
</script>

<div class="strip">
  <span class="hint">改期：把待办拖到下面任意一天</span>
  <div class="days">
    {#each days as ymd (ymd)}
      {@const i = info(ymd)}
      <!-- a11y 忽略：这里是拖放落点，键盘用户走详情面板的日期选择器 -->
      <div
        class="day"
        data-ymd={ymd}
        class:hover={drag.hoverDate === ymd}
        class:hol={i.mark.holiday}
        class:work={i.mark.workday}
        class:weekend={i.isWeekend && !i.mark.holiday && !i.mark.workday}
        title={i.mark.holiday ? `${i.mark.name ?? "法定假日"} · 拖到此处改期` : "拖到此处改期"}
      >
        <span class="d">{i.d}</span>
        <span class="w">周{WEEK[i.wd]}</span>
        {#if i.mark.holiday}
          <span class="tag">{i.mark.name ?? "休"}</span>
        {:else if i.mark.workday}
          <span class="tag work-tag">班</span>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .strip {
    padding: 8px 18px;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }
  .hint { font-size: 11px; color: var(--color-text-dim); }
  .days { display: flex; gap: 6px; margin-top: 6px; }
  .day {
    flex: 1;
    min-width: 0;
    border: 1px dashed var(--color-border);
    border-radius: 6px;
    padding: 4px 2px;
    text-align: center;
    font-size: 11px;
    color: var(--color-text-dim);
    display: flex;
    flex-direction: column;
    gap: 1px;
    transition: border-color 0.1s, background 0.1s;
  }
  .day.weekend { color: var(--color-text-dim); }
  .day.hol { background: rgba(217, 75, 75, 0.10); border-style: solid; }
  .day.work { background: rgba(136, 135, 128, 0.12); border-style: solid; }
  .day.hover {
    border-color: var(--color-primary);
    border-style: solid;
    background: var(--color-primary-soft);
    color: var(--color-text);
  }
  .d { font-size: 14px; font-weight: 600; }
  .w { font-size: 10px; }
  .tag { font-size: 9px; color: #e0685f; line-height: 1.1; }
  .work-tag { color: var(--color-text-dim); }
</style>
