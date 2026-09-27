<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { api, type Assignee, type Goal, type Person, type Project, type Todo } from "../api";
  import { ui } from "../stores.svelte";
  import { loadPeople, peopleState } from "../people.svelte";
  import { loadProjects, projectsState } from "../projects.svelte";
  import { goalsState, loadGoals } from "../goals.svelte";
  import { todosState } from "../todos.svelte";
  import { setItemTags } from "../tags.svelte";
  import LinkPanel from "./LinkPanel.svelte";

  const COLORS = ["red", "orange", "yellow", "green", "blue", "purple", "gray"];
  const COLOR_HEX: Record<string, string> = {
    red: "#ef4444", orange: "#f97316", yellow: "#eab308", green: "#22c55e",
    blue: "#3b82f6", purple: "#a855f7", gray: "#9ca3af",
  };

  // Esc 关闭右栏详情面板（不劫持内联编辑：焦点在 <input> 里时不响应）
  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (!ui.activeTodoId) return;
    const el = e.target as HTMLElement | null;
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT")) return;
    ui.activeTodoId = null;
  }

  let todo = $state<Todo | null>(null);
  let descDraft = $state("");
  let tagDraft = $state("");
  let assignees = $state<Assignee[]>([]);
  /** 该待办被哪些目标关联 */
  let linkedGoalIds = $state<Set<string>>(new Set());

  // 选中项变化 → 拉详情
  $effect(() => {
    const id = ui.activeTodoId;
    if (!id) {
      todo = null;
      return;
    }
    void load(id);
  });

  async function load(id: string) {
    const [t, people, projects, goals] = await Promise.all([
      api.todoGet(id),
      loadPeople(),
      loadProjects(),
      loadGoals(),
    ]);
    if (ui.activeTodoId !== id) return; // 已切换到别的项
    todo = t;
    descDraft = t?.content ?? "";
    tagDraft = (t?.tags ?? []).join(", ");
    assignees = t ? await api.todoGetAssignees(t.id) : [];
    // 该待办被哪些目标关联
    const set = new Set<string>();
    for (const g of goalsState.list) {
      const links = await api.goalGetLinks(g.id);
      if (links.some((l) => l.itemType === "todo" && l.itemId === id)) set.add(g.id);
    }
    linkedGoalIds = set;
  }

  /** 保存后同步列表中的该条 */
  function syncList(updated: Todo) {
    todo = updated;
    todosState.items = todosState.items.map((x) => (x.id === updated.id ? updated : x));
  }

  async function patch(input: Parameters<typeof api.todoUpdate>[1]) {
    if (!todo) return;
    try {
      syncList(await api.todoUpdate(todo.id, input));
    } catch (e) {
      alert(`保存失败：${e}`);
    }
  }

  async function saveTitle(e: Event) {
    const title = (e.target as HTMLInputElement).value.trim();
    if (!todo || !title || title === todo.title) return;
    await patch({ title });
  }
  async function saveDesc() {
    if (!todo) return;
    const content = descDraft;
    if (content === (todo.content ?? "")) return;
    // 清空发 ""（后端映射为 NULL）；发 null 会被 serde 折叠成"不改"
    await patch({ content });
  }
  async function saveTags() {
    if (!todo) return;
    const names = tagDraft.split(/[,，]/).map((s) => s.trim()).filter(Boolean);
    if (names.join("|") === (todo.tags ?? []).join("|")) return;
    try {
      await setItemTags("todo", todo.id, names);
      syncList(await api.todoGet(todo.id) as Todo);
    } catch (e) {
      alert(`标签保存失败：${e}`);
    }
  }

  // ---- 分配与协作完成 ----
  function isAssigned(personId: string) {
    return assignees.some((a) => a.personId === personId);
  }
  function markOf(personId: string) {
    return assignees.find((a) => a.personId === personId)?.completed ?? false;
  }
  async function toggleAssign(p: Person, join: boolean) {
    if (!todo) return;
    const ids = join
      ? [...new Set([...assignees.map((a) => a.personId), p.id])]
      : assignees.map((a) => a.personId).filter((x) => x !== p.id);
    try {
      assignees = await api.todoSetAssignees(todo.id, ids);
      syncList((await api.todoGet(todo.id)) as Todo);
    } catch (e) {
      alert(`分配失败：${e}`);
    }
  }
  /** 按人勾选（协作完成语义：全部勾选才整体完成） */
  async function togglePersonMark(a: Assignee, completed: boolean) {
    if (!todo) return;
    try {
      assignees = await api.todoPersonToggle(todo.id, a.personId, completed);
      syncList((await api.todoGet(todo.id)) as Todo);
    } catch (e) {
      alert(`勾选失败：${e}`);
    }
  }

  // ---- 目标关联 ----
  async function toggleGoal(g: Goal, join: boolean) {
    if (!todo) return;
    const current = (await api.goalGetLinks(g.id))
      .filter((l) => !(l.itemType === "todo" && l.itemId === todo!.id))
      .map((l) => ({ itemType: l.itemType, itemId: l.itemId }));
    const items = join ? [...current, { itemType: "todo" as const, itemId: todo.id }] : current;
    try {
      await api.goalSetLinks(g.id, items);
      const next = new Set(linkedGoalIds);
      if (join) next.add(g.id);
      else next.delete(g.id);
      linkedGoalIds = next;
    } catch (e) {
      alert(`目标关联失败：${e}`);
    }
  }

  async function removeSelf() {
    if (!todo || !window.confirm(`删除待办「${todo.title}」？`)) return;
    try {
      await api.todoDelete(todo.id);
      todosState.items = todosState.items.filter((x) => x.id !== todo!.id);
      ui.activeTodoId = null;
    } catch (e) {
      alert(`删除失败：${e}`);
    }
  }

  // T3.5 重复规则（每日/每周/每月）
  type RepeatFreq = "daily" | "weekly" | "monthly";
  let repeatFreq = $state<RepeatFreq | "">("");
  let repeatDays = $state<number[]>([]);

  async function loadRepeat() {
    if (!todo) return;
    repeatFreq = "";
    repeatDays = [];
    try {
      const r = await (await import("@tauri-apps/api/core")).invoke<{
        freq: string;
        days: number[];
        startDate: number;
      } | null>("repeat_get", { todoId: todo.id });
      if (r) {
        repeatFreq = r.freq as RepeatFreq;
        repeatDays = r.days ?? [];
      }
    } catch {
      /* ignore */
    }
  }
  $effect(() => {
    void todo?.id;
    void loadRepeat();
  });
  async function onRepeatFreq(f: RepeatFreq | "") {
    repeatFreq = f;
    if (!f) {
      await invoke("repeat_clear", { todoId: todo!.id });
      return;
    }
    const days = f === "weekly" ? (repeatDays.length ? repeatDays : [1]) : f === "monthly" ? [1] : [];
    await invoke("repeat_set", {
      todoId: todo!.id,
      freq: f,
      days,
      startDate: todo!.date ?? todayYmdLocal(),
      endDate: null,
    });
    await invoke("repeat_generate");
  }
  function toggleWeekday(d: number) {
    repeatDays = repeatDays.includes(d) ? repeatDays.filter((x) => x !== d) : [...repeatDays, d];
    if (repeatFreq === "weekly") void invoke("repeat_set", {
      todoId: todo!.id,
      freq: "weekly",
      days: repeatDays,
      startDate: todo!.date ?? todayYmdLocal(),
      endDate: null,
    }).then(() => invoke("repeat_generate"));
  }
  function todayYmdLocal(): number {
    const d = new Date();
    return Number(`${d.getFullYear()}${`${d.getMonth() + 1}`.padStart(2, "0")}${`${d.getDate()}`.padStart(2, "0")}`);
  }

  // 日期/时间与 input 值互转
  function ymdToInput(n?: number | null) {
    if (!n) return "";
    const s = String(n);
    return `${s.slice(0, 4)}-${s.slice(4, 6)}-${s.slice(6, 8)}`;
  }
  function inputToYmd(s: string): number | null {
    return s ? Number(s.replaceAll("-", "")) : null;
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

{#if todo}
  <div class="detail">
    <button
      class="close-btn"
      onclick={() => (ui.activeTodoId = null)}
      title="关闭详情面板（Esc）"
      aria-label="关闭详情面板"
    >✕</button>
    <input class="title-input" value={todo.title} onblur={saveTitle}
      onkeydown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />

    <textarea
      class="desc"
      placeholder="描述（失焦自动保存）…"
      bind:value={descDraft}
      onblur={saveDesc}
      rows="5"
    ></textarea>

    <div class="grid">
      <label>分类
        <select value={todo.category} onchange={(e) => patch({ category: (e.target as HTMLSelectElement).value as Todo["category"] })}>
          <option value="work">工作</option>
          <option value="life">生活</option>
        </select>
      </label>
      <label>优先级
        <select value={todo.priority} onchange={(e) => patch({ priority: (e.target as HTMLSelectElement).value as Todo["priority"] })}>
          <option value="high">高</option>
          <option value="medium">中</option>
          <option value="low">低</option>
        </select>
      </label>
      <label>日期
        <input type="date" value={ymdToInput(todo.date)}
          onchange={(e) => patch({ date: inputToYmd((e.target as HTMLInputElement).value) ?? 0 })} />
      </label>
      <label>时间
        <input type="time" value={todo.time ?? ""}
          onchange={(e) => patch({ time: (e.target as HTMLInputElement).value })} />
      </label>
      <label class="wide">重复（F15：每日/每周/每月，生成实例）
        <select value={repeatFreq} onchange={(e) => void onRepeatFreq((e.target as HTMLSelectElement).value as RepeatFreq)}>
          <option value="">不重复</option>
          <option value="daily">每天</option>
          <option value="weekly">每周</option>
          <option value="monthly">每月</option>
        </select>
        {#if repeatFreq === "weekly"}
          <span class="weekdays">
            {#each ["日", "一", "二", "三", "四", "五", "六"] as wd, i}
              <button class="wd" class:on={repeatDays.includes(i)} onclick={() => toggleWeekday(i)}>{wd}</button>
            {/each}
          </span>
        {/if}
      </label>
      <label class="wide">项目
        <select value={todo.projectId ?? ""}
          onchange={(e) => patch({ projectId: (e.target as HTMLSelectElement).value })}>
          <option value="">（无）</option>
          {#each projectsState.list as p (p.id)}
            <option value={p.id}>{p.name}</option>
          {/each}
        </select>
      </label>
    </div>

    <div class="sec-title">颜色</div>
    <div class="colors">
      {#each COLORS as c (c)}
        <button
          class="dot"
          class:on={todo.color === c}
          style={`background:${COLOR_HEX[c]}`}
          onclick={() => patch({ color: todo!.color === c ? "" : c })}
          title={c}
        ></button>
      {/each}
    </div>

    <div class="sec-title">标签（逗号分隔，失焦保存）</div>
    <input type="text" bind:value={tagDraft} onblur={saveTags}
      onkeydown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />

    <!-- 人员分配（仅工作任务，F31） -->
    {#if todo.category === "work"}
      <div class="sec-title">人员分配（全部完成才整体完成）</div>
      {#if peopleState.list.length === 0}
        <p class="dim">暂无人员，请在「人员」页新建。</p>
      {:else}
        <ul class="people">
          {#each peopleState.list as p (p.id)}
            <li>
              <!-- click 而非 change：checkbox 原生先翻转 checked 再派发 click，实测 click 委托更可靠 -->
              <input
                type="checkbox"
                checked={isAssigned(p.id)}
                onclick={(e) => toggleAssign(p, (e.target as HTMLInputElement).checked)}
                title="分配/取消分配"
              />
              <span class="pname">{p.name}</span>
              {#if isAssigned(p.id)}
                <label class="mark">
                  <input
                    type="checkbox"
                    checked={markOf(p.id)}
                    onclick={(e) =>
                      togglePersonMark(
                        assignees.find((a) => a.personId === p.id)!,
                        (e.target as HTMLInputElement).checked,
                      )}
                  />
                  已完成
                </label>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}

    <!-- 目标关联 -->
    <div class="sec-title">长期目标</div>
    {#if goalsState.list.length === 0}
      <p class="dim">暂无目标，请在「目标」页新建。</p>
    {:else}
      <ul class="goals">
        {#each goalsState.list as g (g.id)}
          <li>
            <!-- click 而非 change：同人员 checkbox，规避 change 委托在该面板的触发问题 -->
            <input
              type="checkbox"
              checked={linkedGoalIds.has(g.id)}
              onclick={(e) => toggleGoal(g, (e.target as HTMLInputElement).checked)}
            />
            {g.title}
          </li>
        {/each}
      </ul>
    {/if}

    <!-- 双向链接区（T2.4 LinkPanel） -->
    <LinkPanel itemType="todo" itemId={todo.id} />

    <button class="del" onclick={removeSelf}>删除此待办</button>
  </div>
{:else}
  <div class="empty">
    <p>未选中待办</p>
    <p class="hint">点击列表右侧 › 查看与编辑详情</p>
  </div>
{/if}

<style>
  .detail { position: relative; padding: 14px 16px; overflow-y: auto; flex: 1; display: flex; flex-direction: column; gap: 10px; }
  .close-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    z-index: 5;
    width: 22px;
    height: 22px;
    line-height: 1;
    padding: 0;
    border-radius: 50%;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-text-dim);
    font-size: 12px;
    cursor: pointer;
  }
  .close-btn:hover { color: var(--color-primary); border-color: var(--color-primary); }
  .title-input { font-size: 16px; font-weight: 600; width: 100%; }
  .desc { width: 100%; resize: vertical; font-family: inherit; font-size: 13px;
    padding: 8px; border: 1px solid var(--color-border); border-radius: 6px;
    background: var(--color-surface); color: var(--color-text); }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .grid label { display: flex; flex-direction: column; gap: 3px; font-size: 12px; color: var(--color-text-dim); }
  .grid .wide { grid-column: span 2; }
  .sec-title { font-size: 12px; font-weight: 600; color: var(--color-text-dim); margin-top: 4px; }
  .colors { display: flex; gap: 6px; }
  .dot { width: 18px; height: 18px; border-radius: 50%; border: 2px solid transparent; padding: 0; }
  .dot.on { border-color: var(--color-text); }
  .people, .goals { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .people li, .goals li { display: flex; align-items: center; gap: 8px; font-size: 13px; }
  .pname { flex: 1; }
  .mark { display: inline-flex; align-items: center; gap: 4px; font-size: 12px; color: var(--color-text-dim); }
  .weekdays { display: inline-flex; gap: 4px; margin-top: 4px; flex-wrap: wrap; }
  .wd { font-size: 11px; padding: 1px 6px; border-radius: 4px; }
  .wd.on { background: var(--color-primary); color: var(--color-primary-fg); border-color: var(--color-primary); }
  .del { color: #dc2626; align-self: flex-start; margin-top: 8px; }
  .dim { color: var(--color-text-dim); font-size: 12px; }
  .empty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; color: var(--color-text-dim); gap: 6px; }
  .empty p { margin: 0; }
  .hint { font-size: 12px; opacity: 0.75; }
</style>
