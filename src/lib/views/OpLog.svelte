<script lang="ts">
  // 最近操作只读日志（#32）：回看「我刚才动了什么」——只记不撤。
  // 撤销/回滚是另一套快照机制，风险高、收益低，本版刻意不做，界面也不提供任何回退按钮。
  import { onMount } from "svelte";
  import { api, type OpLogEntry } from "../api";

  let entries = $state<OpLogEntry[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  const ACTION_LABEL: Record<string, string> = {
    create: "新增",
    update: "修改",
    delete: "删除",
    complete: "完成",
    uncomplete: "取消完成",
    archive: "归档",
    unarchive: "取消归档",
    import: "导入",
  };
  const ENTITY_LABEL: Record<string, string> = {
    todo: "待办",
    project: "项目",
    memo: "备忘",
    note: "笔记",
    goal: "目标",
    person: "人员",
  };
  const actionLabel = (a: string) => ACTION_LABEL[a] ?? a;
  const entityLabel = (t: string) => ENTITY_LABEL[t] ?? t;

  // 相对时间：刚刚 / N 分钟前 / N 小时前 / N 天前 / 具体日期
  function relTime(ms: number): string {
    const diff = Date.now() - ms;
    const min = Math.floor(diff / 60000);
    if (min < 1) return "刚刚";
    if (min < 60) return `${min} 分钟前`;
    const hr = Math.floor(min / 60);
    if (hr < 24) return `${hr} 小时前`;
    const day = Math.floor(hr / 24);
    if (day < 7) return `${day} 天前`;
    const d = new Date(ms);
    return `${d.getFullYear()}-${`${d.getMonth() + 1}`.padStart(2, "0")}-${`${d.getDate()}`.padStart(2, "0")} ${`${d.getHours()}`.padStart(2, "0")}:${`${d.getMinutes()}`.padStart(2, "0")}`;
  }

  async function load() {
    loading = true;
    error = null;
    try {
      entries = await api.oplogList(200);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(load);
</script>

<div class="oplog">
  <div class="bar">
    <p class="note">仅记录你通过界面进行的主要增删改，供回看；本视图只读，不提供撤销。</p>
    <button class="mini" onclick={load} disabled={loading}>
      {loading ? "刷新中…" : "刷新"}
    </button>
  </div>

  {#if error}
    <p class="error">读取失败：{error}</p>
  {:else if entries.length === 0}
    <p class="empty">暂无操作记录。你在待办、项目、备忘、笔记、目标、人员上的改动会出现在这里。</p>
  {:else}
    <ul class="list">
      {#each entries as e (e.id)}
        <li class="row">
          <span class="chip act act-{e.action}">{actionLabel(e.action)}</span>
          <span class="chip ent">{entityLabel(e.entityType)}</span>
          <span class="summary" title={e.summary}>{e.summary}</span>
          <span class="time">{relTime(e.ts)}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .oplog {
    padding: 10px 18px 14px;
    overflow-y: auto;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .note {
    margin: 0;
    font-size: 11px;
    color: var(--color-text-dim);
  }
  .mini {
    font-size: 12px;
    padding: 2px 10px;
    white-space: nowrap;
  }
  .empty,
  .error {
    color: var(--color-text-dim);
    text-align: center;
    margin-top: 8vh;
    font-size: 13px;
  }
  .error {
    color: #dc2626;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 8px 12px;
  }
  .chip {
    font-size: 11px;
    padding: 0 8px;
    border-radius: 10px;
    white-space: nowrap;
    border: 1px solid var(--color-border);
    color: var(--color-text-dim);
  }
  .act {
    color: #fff;
    border: none;
    background: #6b7280;
  }
  .act-create,
  .act-import {
    background: #16a34a;
  }
  .act-update {
    background: #2563eb;
  }
  .act-complete {
    background: #0d9488;
  }
  .act-uncomplete,
  .act-unarchive {
    background: #d97706;
  }
  .act-delete,
  .act-archive {
    background: #dc2626;
  }
  .ent {
    background: var(--color-primary-soft);
    color: var(--color-primary);
    border-color: var(--color-primary);
  }
  .summary {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time {
    font-size: 11px;
    color: var(--color-text-dim);
    white-space: nowrap;
  }
</style>
