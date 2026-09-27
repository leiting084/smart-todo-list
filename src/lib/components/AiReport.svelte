<script lang="ts">
  /**
   * AI 周报（F26 / T5.1）：先勾选再生成 → 流式渲染 → 一键存为笔记。
   *
   * 数据不出本机之外的第三方：请求直连用户在设置页填的 base_url，key 自备。
   * 生成前必须用户显式勾选发送范围（学 External 的 select-todos → generate 两步流程），
   * 未勾选项不会出现在请求体里。
   */
  import { onDestroy } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { api, type Todo, type Memo } from "../api";

  let stage = $state<"idle" | "select" | "streaming" | "done">("idle");
  let todos = $state<Todo[]>([]);
  let memos = $state<Memo[]>([]);
  let overdue = $state<Todo[]>([]);
  let selected = $state<Set<string>>(new Set());
  let useMemos = $state(false);
  let text = $state("");
  let error = $state("");
  let busy = $state(false);
  let savedNoteId = $state("");
  let unlisteners: UnlistenFn[] = [];

  function ymdAddDays(ymd: number, n: number): number {
    const y = Math.floor(ymd / 10000);
    const m = Math.floor((ymd % 10000) / 100);
    const d = ymd % 100;
    const dt = new Date(y, m - 1, d + n);
    return dt.getFullYear() * 10000 + (dt.getMonth() + 1) * 100 + dt.getDate();
  }

  function fmt(ymd: number) {
    const m = Math.floor((ymd % 10000) / 100);
    const d = ymd % 100;
    return `${m}/${d}`;
  }

  async function openSelector() {
    error = "";
    busy = true;
    try {
      const today = await api.todayDate();
      const from = ymdAddDays(today, -6);
      const done: Todo[] = [];
      const pending: Todo[] = [];
      for (let i = 0; i < 7; i++) {
        const d = ymdAddDays(from, i);
        const [c, p] = await Promise.all([
          api.todoList({ date: d, completed: true }),
          api.todoList({ date: d, completed: false }),
        ]);
        done.push(...c);
        pending.push(...p);
      }
      todos = done;
      // 逾期未完成 = 近 7 天内日期早于今天且仍未完成的（收集箱无日期的不算逾期）
      overdue = pending.filter((t) => t.date != null && t.date < today);
      memos = await api.memoList(false);
      // 默认全选已完成待办，备忘与逾期默认不勾（学 External：笔记默认不进请求）
      selected = new Set(done.map((t) => t.id));
      useMemos = false;
      stage = "select";
    } catch (e) {
      error = `读取本周数据失败：${e}`;
    } finally {
      busy = false;
    }
  }

  function toggle(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  function buildPrompt() {
    const picked = todos.filter((t) => selected.has(t.id));
    const byDate = new Map<number, Todo[]>();
    for (const t of picked) {
      const k = t.date ?? 0;
      if (!byDate.has(k)) byDate.set(k, []);
      byDate.get(k)!.push(t);
    }
    const lines: string[] = ["以下是本周工作记录，请据此撰写周报："];
    for (const [d, list] of [...byDate.entries()].sort((a, b) => a[0] - b[0])) {
      lines.push(`\n## ${d === 0 ? "收集箱" : fmt(d)}`);
      for (const t of list) lines.push(`- [已完成] ${t.title}`);
    }
    if (useMemos && memos.length) {
      lines.push("\n## 备忘");
      for (const m of memos.slice(0, 30)) lines.push(`- ${m.content ?? ""}`);
    }
    const pickedOverdue = overdue.filter((t) => selected.has(t.id));
    if (pickedOverdue.length) {
      lines.push("\n## 逾期未完成");
      for (const t of pickedOverdue) lines.push(`- ${t.title}`);
    }
    return lines.join("\n");
  }

  async function generate() {
    error = "";
    text = "";
    savedNoteId = "";
    stage = "streaming";
    busy = true;

    // 事件订阅必须在发起请求之前建立，否则开头的 chunk 会丢
    unlisteners.forEach((u) => u());
    unlisteners = [];
    unlisteners.push(
      await listen<string>("ai://chunk", (e) => (text += e.payload)),
      await listen<string>("ai://done", (e) => {
        text = e.payload;
        stage = "done";
        busy = false;
      }),
      await listen<string>("ai://error", (e) => {
        error = e.payload;
        stage = "done";
        busy = false;
      }),
    );

    try {
      await api.aiGenerate("", buildPrompt());
    } catch (e) {
      error = `发起生成失败：${e}`;
      stage = "done";
      busy = false;
    }
  }

  async function stop() {
    await api.aiCancel().catch(() => {});
    busy = false;
    stage = "done";
  }

  /** 存为笔记：正文带上 wiki-link，note_create 会自动 diff 建反链（L3 沉淀闭环） */
  async function saveAsNote() {
    if (!text.trim()) return;
    const picked = [...todos, ...overdue].filter((t) => selected.has(t.id));
    const links = picked.slice(0, 50).map((t) => `[[todo:${t.id}]]`).join(" ");
    const today = await api.todayDate();
    try {
      const note = await api.noteCreate({
        title: `周报 ${fmt(ymdAddDays(today, -6))}–${fmt(today)}`,
        content: `${text}\n\n---\n${links}`,
      });
      savedNoteId = note.id;
    } catch (e) {
      error = `存为笔记失败：${e}`;
    }
  }

  onDestroy(() => unlisteners.forEach((u) => u()));
</script>

<section class="ai">
  <header>
    <h4>AI 周报</h4>
    {#if stage === "idle"}
      <button onclick={openSelector} disabled={busy}>
        {busy ? "读取中…" : "选择内容并生成"}
      </button>
    {:else if stage === "select"}
      <button onclick={generate}>生成（已选 {selected.size} 项）</button>
      <button onclick={() => (stage = "idle")}>取消</button>
    {:else if stage === "streaming"}
      <button onclick={stop}>停止</button>
    {:else}
      <button onclick={() => { stage = "idle"; text = ""; }}>关闭</button>
      {#if text.trim()}
        <button onclick={saveAsNote} disabled={!!savedNoteId}>
          {savedNoteId ? "已存为笔记" : "存为笔记"}
        </button>
      {/if}
    {/if}
  </header>

  {#if error}<p class="err">{error}</p>{/if}

  {#if stage === "select"}
    <p class="hint">
      默认勾选近 7 天已完成待办。未勾选项**不会**出现在发送给模型的内容里。
    </p>
    <div class="rows">
      {#each todos as t (t.id)}
        <label><input type="checkbox" checked={selected.has(t.id)} onclick={() => toggle(t.id)} />
        <span>{t.date ? fmt(t.date) : "收集箱"} · {t.title}</span></label>
      {/each}
    </div>
    {#if overdue.length}
      <p class="sub">逾期未完成（默认不勾选）</p>
      <div class="rows">
        {#each overdue as t (t.id)}
          <label><input type="checkbox" checked={selected.has(t.id)} onclick={() => toggle(t.id)} />
          <span>{t.date ? fmt(t.date) : ""} · {t.title}</span></label>
        {/each}
      </div>
    {/if}
    {#if memos.length}
      <label class="memo">
        <input type="checkbox" checked={useMemos} onclick={() => (useMemos = !useMemos)} />
        <span>附带 {memos.length} 条备忘（默认不带）</span>
      </label>
    {/if}
  {/if}

  {#if stage === "streaming" || stage === "done"}
    <pre class="out">{text || "等待模型返回…"}</pre>
  {/if}
</section>

<style>
  .ai { border-top: 1px solid var(--color-border); padding-top: 10px; margin-top: 12px; }
  header { display: flex; align-items: center; gap: 8px; }
  h4 { margin: 0; font-size: 13px; font-weight: 600; }
  header button { font-size: 12px; padding: 2px 10px; }
  .hint, .sub { font-size: 11px; color: var(--color-text-dim); margin: 6px 0; }
  .sub { margin-top: 10px; }
  .rows { max-height: 180px; overflow-y: auto; display: flex; flex-direction: column; gap: 2px; }
  .rows label, .memo { display: flex; align-items: center; gap: 6px; font-size: 12px; }
  .rows span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .out {
    max-height: 260px; overflow-y: auto; white-space: pre-wrap; word-break: break-word;
    background: var(--color-surface); border: 1px solid var(--color-border);
    border-radius: 6px; padding: 8px 10px; font-size: 12px; line-height: 1.6;
    margin: 8px 0 0;
  }
  .err { color: #e0685f; font-size: 12px; margin: 6px 0; }
</style>
