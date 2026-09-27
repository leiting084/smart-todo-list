<script lang="ts">
  import type { Assignee, BatchTodoPatch, Todo } from "../api";
  import { api } from "../api";
  import {
    localTodayYmd,
    removeTodo,
    scheduleTodo,
    todosState,
    toggleComplete,
    updateTodo,
  } from "../todos.svelte";
  import { setItemTags } from "../tags.svelte";
  import { ui } from "../stores.svelte";
  import { drag } from "../drag.svelte";
  import { loadPeople, peopleState } from "../people.svelte";
  import { convertTodoToMemo } from "../memos.svelte";

  // ---- T3.1 子待办：树化展开（折叠状态本地） ----
  let collapsed = $state<Set<string>>(new Set());
  function toggleCollapse(id: string) {
    const next = new Set(collapsed);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    collapsed = next;
  }
  function childrenOf(id: string) {
    return todosState.items.filter((t) => t.parentId === id);
  }
  /** 按树序展开为 (todo, depth) 平铺；折叠节点的后代不展开 */
  const flatTree = $derived.by(() => {
    const out: { t: (typeof todosState.items)[number]; depth: number }[] = [];
    const walk = (parentId: string | null, depth: number) => {
      for (const t of todosState.items.filter((x) => (x.parentId ?? null) === parentId)) {
        out.push({ t, depth });
        if (!collapsed.has(t.id)) walk(t.id, depth + 1);
      }
    };
    walk(null, 0);
    return out;
  });

  /** 新建子待办（行内"＋子"按钮 / 编辑态 Shift+Enter） */
  async function addChild(parent: (typeof todosState.items)[number]) {
    try {
      const child = await api.todoCreate({
        title: "新子任务",
        parentId: parent.id,
        date: parent.date ?? null,
        category: parent.category,
      });
      todosState.items = [...todosState.items, child];
      editingId = child.id;
      editingText = child.title;
      collapsed.delete(parent.id);
    } catch (e) {
      alert(`创建子任务失败：${e}`);
    }
  }

  /** 勾选父项时若有未完成子任务 → 询问是否连带完成（默认仅父） */
  async function onToggleWithChildren(t: (typeof todosState.items)[number]) {
    const children = childrenOf(t.id);
    const willComplete = !t.completed;
    if (willComplete && children.some((c) => !c.completed)) {
      if (window.confirm(`「${t.title}」有未完成的子任务，连同子任务一起完成？\n（取消=仅完成父任务）`)) {
        try {
          await api.todoCompleteWithChildren(t.id);
          const fresh = await api.todoList({ date: t.date ?? undefined, inbox: t.date ? undefined : true }).catch(() => null);
          if (fresh) todosState.items = fresh;
        } catch (e) {
          alert(`连带完成失败：${e}`);
        }
        return;
      }
    }
    await onToggle(t);
  }

  // 7 色（SPEC §5）
  const COLORS = ["red", "orange", "yellow", "green", "blue", "purple", "gray"];
  const COLOR_HEX: Record<string, string> = {
    red: "#ef4444",
    orange: "#f97316",
    yellow: "#eab308",
    green: "#22c55e",
    blue: "#3b82f6",
    purple: "#a855f7",
    gray: "#9ca3af",
  };
  const PRIORITY_LABEL: Record<string, string> = { high: "高", medium: "中", low: "低" };

  let {
    inboxMode = false,
    reorderable = true,
    emptyText = "暂无待办",
    onReorder,
    onRefresh,
  }: {
    inboxMode?: boolean;
    reorderable?: boolean;
    emptyText?: string;
    onReorder: (ids: string[]) => Promise<void> | void;
    /** 分配变更等需要整表重拉时调用（各视图传自己的 reload） */
    onRefresh?: () => Promise<void> | void;
  } = $props();

  let editingId = $state<string | null>(null);
  let editingText = $state("");
  let colorPickerId = $state<string | null>(null);
  let dragId = $state<string | null>(null);
  // ---- 指针拖拽换序 / 拖到日期改期（绕开 WebView2 不可靠的 HTML5 原生拖拽）----
  let dropTargetId = $state<string | null>(null);
  let dropBelow = $state(false);
  let pointerDownId: string | null = null;
  let draggingActive = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let tagEditorId = $state<string | null>(null);
  let tagDraft = $state("");

  // ---- 人员分配编辑器（仅 work 行，F31） ----
  let assignEditorId = $state<string | null>(null);
  let assignSelection = $state<Set<string>>(new Set());
  let assigneesCache = $state<Map<string, Assignee[]>>(new Map());

  async function openAssignEditor(t: Todo) {
    await loadPeople();
    const current = await api.todoGetAssignees(t.id);
    assigneesCache = new Map(assigneesCache).set(t.id, current);
    assignSelection = new Set(current.map((a) => a.personId));
    assignEditorId = t.id;
  }
  function toggleAssign(personId: string) {
    const next = new Set(assignSelection);
    if (next.has(personId)) next.delete(personId);
    else next.add(personId);
    assignSelection = next;
  }
  async function saveAssignees(t: Todo) {
    if (assignEditorId !== t.id) return;
    assignEditorId = null;
    try {
      const list = await api.todoSetAssignees(t.id, [...assignSelection]);
      assigneesCache = new Map(assigneesCache).set(t.id, list);
      // 协作语义：整体状态由后端按标记重算，整表重拉保证行状态正确
      await onRefresh?.();
    } catch (e) {
      alert(`分配保存失败：${e}`);
    }
  }
  function assigneeCount(t: Todo): number | null {
    const list = assigneesCache.get(t.id);
    return list ? list.length : null;
  }

  // 进入编辑态时聚焦（替代 autofocus，避免 a11y 警告）
  function focusOnMount(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function startEdit(t: Todo) {
    editingId = t.id;
    editingText = t.title;
  }
  async function commitEdit(t: Todo) {
    const title = editingText.trim();
    editingId = null;
    if (title && title !== t.title) {
      try {
        await updateTodo(t.id, { title });
      } catch (e) {
        alert(`保存失败：${e}`);
      }
    }
  }

  async function onToggle(t: Todo) {
    try {
      const before = t.completed;
      await toggleComplete(t);
      // F3：从未完成→完成时弹"转为备忘"确认条（3 秒自动消失=忽略）
      if (!before) showConvertBar(t.id);
    } catch (e) {
      alert(`操作失败：${e}`);
    }
  }

  // ---- 转备忘确认条（3 秒自动消失=忽略） ----
  let convertBarId = $state<string | null>(null);
  let convertBarTimer: ReturnType<typeof setTimeout> | undefined;
  function showConvertBar(todoId: string) {
    convertBarId = todoId;
    clearTimeout(convertBarTimer);
    convertBarTimer = setTimeout(() => (convertBarId = null), 3000);
  }
  async function doConvert(todoId: string | null) {
    clearTimeout(convertBarTimer);
    convertBarId = null;
    if (!todoId) return;
    try {
      await convertTodoToMemo(todoId);
    } catch (e) {
      alert(`转备忘失败：${e}`);
    }
  }

  async function onDelete(t: Todo) {
    if (!window.confirm(`删除待办「${t.title}」？`)) return;
    try {
      await removeTodo(t.id);
      if (ui.activeTodoId === t.id) ui.activeTodoId = null;
    } catch (e) {
      alert(`删除失败：${e}`);
    }
  }

  async function onPriority(t: Todo, priority: string) {
    await updateTodo(t.id, { priority: priority as Todo["priority"] });
  }

  async function onColor(t: Todo, color: string | null) {
    colorPickerId = null;
    await updateTodo(t.id, { color });
  }

  async function onScheduleToday(t: Todo) {
    await scheduleTodo(t.id, todosState.today || null);
  }

  function startTagEdit(t: Todo) {
    tagEditorId = t.id;
    tagDraft = (t.tags ?? []).join(", ");
  }
  async function commitTags(t: Todo) {
    if (tagEditorId !== t.id) return;
    tagEditorId = null;
    const names = tagDraft
      .split(/[,，]/)
      .map((s) => s.trim())
      .filter(Boolean);
    const old = t.tags ?? [];
    if (names.join("|") === old.join("|")) return;
    try {
      await setItemTags("todo", t.id, names);
    } catch (e) {
      alert(`标签保存失败：${e}`);
    }
  }

  // ---- 拖拽排序（仅未完成分区）----
  function canRowDrag(t: Todo, depth: number): boolean {
    return reorderable && !t.completed && depth === 0 && !batchMode;
  }

  function resetDragState() {
    pointerDownId = null;
    draggingActive = false;
    dragId = null;
    dropTargetId = null;
    dropBelow = false;
    drag.draggingId = null;
    drag.hoverDate = 0;
  }

  function onRowPointerDown(e: PointerEvent, t: Todo, depth: number) {
    if (!canRowDrag(t, depth)) return;
    if (e.button !== 0) return;
    const tgt = e.target as HTMLElement | null;
    // 落在交互控件上的按下交给控件自身（复选/按钮/下拉/输入/标签等），不起拖
    if (tgt && tgt.closest("button,input,select,textarea,a,label")) return;
    pointerDownId = t.id;
    draggingActive = false;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    // 用 window 级监听：指针移出本行后仍能持续收到 move/up，
    // 否则快速拖到别的行时，move 落在目标行、本行 handler 因 id 不符被跳过 → 拖不动。
    window.addEventListener("pointermove", onWindowPointerMove);
    window.addEventListener("pointerup", onWindowPointerUp);
    window.addEventListener("pointercancel", onWindowPointerCancel);
  }

  function detachDragListeners() {
    window.removeEventListener("pointermove", onWindowPointerMove);
    window.removeEventListener("pointerup", onWindowPointerUp);
    window.removeEventListener("pointercancel", onWindowPointerCancel);
  }

  function onWindowPointerMove(e: PointerEvent) {
    if (!pointerDownId) return;
    if (!draggingActive) {
      // 超过阈值才算拖拽；此前保持普通点击语义
      if (Math.abs(e.clientX - dragStartX) < 4 && Math.abs(e.clientY - dragStartY) < 4) return;
      draggingActive = true;
      dragId = pointerDownId;
      drag.draggingId = pointerDownId;
    }
    e.preventDefault();
    const under = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
    // 1) 悬停到日期栏某格 → 改期落点
    const dayEl = under?.closest?.("[data-ymd]") as HTMLElement | null;
    if (dayEl) {
      const ymd = Number(dayEl.getAttribute("data-ymd"));
      drag.hoverDate = ymd || 0;
      dropTargetId = null;
      return;
    }
    drag.hoverDate = 0;
    // 2) 悬停到另一行 → 换序落点（按上下半区决定插到其前/后）
    const rowEl = under?.closest?.(".todo-row[data-todo-id]") as HTMLElement | null;
    const id = rowEl?.getAttribute("data-todo-id") ?? null;
    if (id && id !== pointerDownId && rowEl) {
      const rect = rowEl.getBoundingClientRect();
      dropBelow = e.clientY - rect.top > rect.height / 2;
      dropTargetId = id;
    } else {
      dropTargetId = null;
    }
  }

  async function onWindowPointerUp(e: PointerEvent) {
    detachDragListeners();
    if (!pointerDownId) return;
    const wasDragging = draggingActive;
    const moving = pointerDownId;
    const targetDate = drag.hoverDate;
    const targetRow = dropTargetId;
    const below = dropBelow;
    resetDragState();
    if (!wasDragging) return; // 只是点击，不处理
    void e;
    if (targetDate) {
      await rescheduleTodo(moving, targetDate);
      return;
    }
    if (targetRow) await applyReorder(moving, targetRow, below);
  }

  function onWindowPointerCancel(e: PointerEvent) {
    detachDragListeners();
    void e;
    resetDragState();
  }

  async function applyReorder(moving: string, targetId: string, below: boolean) {
    if (moving === targetId) return;
    const active = todosState.items.filter((x) => !x.completed);
    const ids = active.map((x) => x.id);
    const from = ids.indexOf(moving);
    if (from < 0) return;
    ids.splice(from, 1);
    let to = ids.indexOf(targetId);
    if (to < 0) return;
    if (below) to += 1;
    ids.splice(to, 0, moving);
    // 乐观重排后持久化
    const snapshot = [...todosState.items];
    const byId = new Map(snapshot.map((x) => [x.id, x]));
    todosState.items = [
      ...ids.map((id) => byId.get(id)!),
      ...todosState.items.filter((x) => x.completed),
    ];
    try {
      await onReorder(ids);
    } catch (err) {
      todosState.items = snapshot; // 回滚
      alert(`排序失败：${err}`);
    }
  }

  async function rescheduleTodo(id: string, ymd: number) {
    try {
      await api.todoUpdate(id, { date: ymd });
      await onRefresh?.();
    } catch (err) {
      alert(`改期失败：${err}`);
    }
  }

  // ---- 多选批量操作 ----
  let batchMode = $state(false);
  let selected = $state<Set<string>>(new Set());
  let batchPickDate = $state("");

  const allSelected = $derived(
    flatTree.length > 0 && flatTree.every(({ t }) => selected.has(t.id)),
  );

  function enterBatch() {
    editingId = null;
    colorPickerId = null;
    tagEditorId = null;
    assignEditorId = null;
    batchMode = true;
  }
  function exitBatch() {
    batchMode = false;
    selected = new Set();
    batchPickDate = "";
  }
  function toggleSelect(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }
  function toggleSelectAll() {
    selected = allSelected ? new Set() : new Set(flatTree.map(({ t }) => t.id));
  }

  /** 今天的 YYYYMMDD：优先用后端认定的今天，未加载时退回本地日期 */
  function todayYmd(): number {
    return todosState.today || localTodayYmd();
  }
  /** YYYYMMDD + n 天（跨月/跨年安全） */
  function ymdAddDays(ymd: number, days: number): number {
    const s = String(ymd);
    const d = new Date(Number(s.slice(0, 4)), Number(s.slice(4, 6)) - 1, Number(s.slice(6, 8)));
    d.setDate(d.getDate() + days);
    const y = d.getFullYear();
    const m = `${d.getMonth() + 1}`.padStart(2, "0");
    const dd = `${d.getDate()}`.padStart(2, "0");
    return Number(`${y}${m}${dd}`);
  }
  function dateStrToYmd(s: string): number | null {
    if (!s) return null;
    const n = Number(s.replaceAll("-", ""));
    return Number.isFinite(n) && n > 0 ? n : null;
  }

  async function runBatch(patch: BatchTodoPatch) {
    const ids = [...selected];
    if (ids.length === 0) return;
    try {
      await api.todoBatchUpdate(ids, patch);
      selected = new Set();
      await onRefresh?.();
    } catch (e) {
      alert(`批量操作失败：${e}`);
    }
  }
  async function batchPickApply() {
    const ymd = dateStrToYmd(batchPickDate);
    if (!ymd) return;
    await runBatch({ date: ymd });
    batchPickDate = "";
  }
  async function batchDelete() {
    const ids = [...selected];
    if (ids.length === 0) return;
    if (!window.confirm(`删除选中的 ${ids.length} 条待办？不可恢复。`)) return;
    try {
      await api.todoBatchDelete(ids);
      if (ui.activeTodoId && ids.includes(ui.activeTodoId)) ui.activeTodoId = null;
      selected = new Set();
      await onRefresh?.();
    } catch (e) {
      alert(`批量删除失败：${e}`);
    }
  }
</script>

<!-- F3 转备忘确认条（3 秒自动消失=忽略，SPEC T2.1） -->
{#if convertBarId}
  <div class="convert-bar">
    <span>转为备忘？</span>
    <button class="primary" onclick={() => doConvert(convertBarId)}>转备忘</button>
    <button class="ignore" onclick={() => (convertBarId = null)}>忽略</button>
  </div>
{/if}

<!-- 批量工具行：进入/退出多选模式 -->
<div class="batch-toolbar">
  {#if !batchMode}
    <button class="mini" onclick={enterBatch} disabled={todosState.items.length === 0} title="进入多选批量操作">
      批量
    </button>
  {:else}
    <button class="mini" onclick={toggleSelectAll}>{allSelected ? "全不选" : "全选"}</button>
    <span class="sel-count">已选 {selected.size} 条</span>
    <button class="mini" onclick={exitBatch}>退出批量</button>
  {/if}
</div>

<ul class="todo-list">
  {#each flatTree as { t, depth } (t.id)}
    <li
      class="todo-row"
      data-todo-id={t.id}
      class:done={t.completed}
      class:sel={batchMode && selected.has(t.id)}
      class:can-drag={reorderable && !t.completed && depth === 0 && !batchMode}
      class:dragging={dragId === t.id}
      class:drop-above={dropTargetId === t.id && !dropBelow}
      class:drop-below={dropTargetId === t.id && dropBelow}
      style={depth > 0 ? `padding-left:${18 + depth * 20}px` : ""}
      onpointerdown={(e) => onRowPointerDown(e, t, depth)}
    >
      <!-- 多选模式：行首复选框（onclick 切换选中，不改完成状态） -->
      {#if batchMode}
        <input
          type="checkbox"
          class="sel-box"
          checked={selected.has(t.id)}
          onclick={() => toggleSelect(t.id)}
          title="选中/取消选中"
        />
      {/if}

      <!-- 折叠/展开（有子任务时） -->
      {#if childrenOf(t.id).length > 0}
        <button class="fold" onclick={() => toggleCollapse(t.id)} title={collapsed.has(t.id) ? "展开" : "折叠"}>
          {collapsed.has(t.id) ? "▸" : "▾"}
        </button>
      {:else}
        <span class="fold-placeholder"></span>
      {/if}

      <input
        type="checkbox"
        checked={t.completed}
        disabled={(assigneeCount(t) ?? 0) > 0}
        title={(assigneeCount(t) ?? 0) > 0 ? "多人任务：在详情面板按人勾选" : "完成/取消（有子任务会询问是否连带）"}
        onchange={() => onToggleWithChildren(t)}
      />

      <!-- 颜色标记 + 7 色选择 -->
      <span class="color-wrap">
        <button
          class="color-dot"
          style={t.color ? `background:${COLOR_HEX[t.color] ?? "transparent"}` : ""}
          title="颜色"
          onclick={() => (colorPickerId = colorPickerId === t.id ? null : t.id)}
        ></button>
        {#if colorPickerId === t.id}
          <span class="color-pop">
            {#each COLORS as c}
              <button
                class="swatch"
                style={`background:${COLOR_HEX[c]}`}
                onclick={() => onColor(t, c)}
                title={c}
              ></button>
            {/each}
            <button class="swatch none" onclick={() => onColor(t, null)} title="无颜色">∅</button>
          </span>
        {/if}
      </span>

      {#if editingId === t.id}
        <input
          class="inline-edit"
          type="text"
          bind:value={editingText}
          use:focusOnMount
          onkeydown={(e) => {
            if (e.key === "Enter") commitEdit(t);
            if (e.key === "Enter" && e.shiftKey) void addChild(t); // T3.1：Shift+Enter 新增子项
            if (e.key === "Escape") editingId = null;
          }}
          onblur={() => commitEdit(t)}
        />
      {:else}
        <!-- 单击进入行内编辑（SPEC §8.3，对标 External 单击编辑习惯）；批量模式下单击=切换选中 -->
        <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
        <span
          class="title"
          role="button"
          tabindex="0"
          onclick={() => (batchMode ? toggleSelect(t.id) : startEdit(t))}
          onkeydown={(e) => e.key === "Enter" && (batchMode ? toggleSelect(t.id) : startEdit(t))}
          title={batchMode ? "点击切换选中" : "单击编辑"}
        >
          {t.title}
        </span>
      {/if}

      <!-- 标签胶囊 + 行内标签编辑 -->
      <span class="tags">
        {#each t.tags ?? [] as name (name)}
          <button class="pill" onclick={() => (ui.activeTag = name)} title="按此标签筛选">
            #{name}
          </button>
        {/each}
        {#if tagEditorId === t.id}
          <input
            class="tag-edit"
            type="text"
            placeholder="逗号分隔"
            bind:value={tagDraft}
            use:focusOnMount
            onkeydown={(e) => {
              if (e.key === "Enter") commitTags(t);
              if (e.key === "Escape") tagEditorId = null;
            }}
            onblur={() => commitTags(t)}
          />
        {:else}
          <button class="tag-add" onclick={() => startTagEdit(t)} title="编辑标签">＋</button>
        {/if}
      </span>

      <span class="spacer"></span>

      <!-- T3.1 新增子任务 -->
      <button class="add-sub" onclick={() => void addChild(t)} title="新增子任务（编辑态 Shift+Enter 同效）">
        ＋子
      </button>

      <!-- 分配（仅工作任务；生活任务隐藏分配区，F31/TASKS T1.6） -->
      {#if t.time}
        <span class="clock" title={`提醒 ${t.time}`}>🕐</span>
      {/if}

      {#if t.category === "work"}
        <button class="assign-btn" onclick={() => openAssignEditor(t)} title="分配人员">
          分配{(assigneeCount(t) ?? 0) > 0 ? `·${assigneeCount(t)}` : ""}
        </button>
        {#if assignEditorId === t.id}
          <span class="assign-pop">
            {#if peopleState.list.length === 0}
              <span class="assign-empty">暂无人员，请先在「人员」页新建</span>
            {:else}
              {#each peopleState.list as p (p.id)}
                <label class="assign-row">
                  <input
                    type="checkbox"
                    checked={assignSelection.has(p.id)}
                    onchange={() => toggleAssign(p.id)}
                  />
                  {p.name}
                </label>
              {/each}
              <button class="primary assign-save" onclick={() => saveAssignees(t)}>
                保存分配
              </button>
            {/if}
          </span>
        {/if}
      {/if}

      {#if inboxMode}
        <button class="mini" onclick={() => onScheduleToday(t)} title="安排到今天">
          安排今天
        </button>
      {/if}

      <select
        class="priority"
        class:p-high={t.priority === "high"}
        value={t.priority}
        onchange={(e) => onPriority(t, (e.target as HTMLSelectElement).value)}
        title="优先级"
      >
        <option value="high">{PRIORITY_LABEL.high}</option>
        <option value="medium">{PRIORITY_LABEL.medium}</option>
        <option value="low">{PRIORITY_LABEL.low}</option>
      </select>

      {#if !batchMode}
        <button
          class="detail-btn"
          class:on={ui.activeTodoId === t.id}
          onclick={() => (ui.activeTodoId = ui.activeTodoId === t.id ? null : t.id)}
          title="详情面板"
        >
          ›
        </button>
        <button class="del" onclick={() => onDelete(t)} title="删除">✕</button>
      {/if}
    </li>
  {/each}
  {#if !todosState.loading && todosState.items.length === 0}
    <li class="empty">{emptyText}</li>
  {/if}
</ul>

<!-- 底部批量操作栏（选中 >0 时出现） -->
{#if batchMode && selected.size > 0}
  <div class="batch-bar">
    <button class="mini" onclick={() => runBatch({ completed: true })} title="批量完成">完成</button>
    <button class="mini" onclick={() => runBatch({ completed: false })} title="批量取消完成">取消完成</button>
    <span class="sep"></span>
    <button class="mini" onclick={() => runBatch({ date: todayYmd() })}>改为今天</button>
    <button class="mini" onclick={() => runBatch({ date: ymdAddDays(todayYmd(), 1) })}>改为明天</button>
    <input
      class="batch-date"
      type="date"
      bind:value={batchPickDate}
      onchange={batchPickApply}
      title="选日期后批量改期"
    />
    <span class="sep"></span>
    <button class="mini" onclick={() => runBatch({ priority: "high" })}>高</button>
    <button class="mini" onclick={() => runBatch({ priority: "medium" })}>中</button>
    <button class="mini" onclick={() => runBatch({ priority: "low" })}>低</button>
    <span class="sep"></span>
    <button class="mini" onclick={() => runBatch({ category: "work" })} title="把选中项分类改为工作">→工作</button>
    <button class="mini" onclick={() => runBatch({ category: "life" })} title="把选中项分类改为生活">→生活</button>
    <span class="sep"></span>
    <span class="colors">
      {#each COLORS as c (c)}
        <button
          class="swatch"
          style={`background:${COLOR_HEX[c]}`}
          onclick={() => runBatch({ color: c })}
          title={c}
        ></button>
      {/each}
    </span>
    <span class="sep"></span>
    <button class="mini danger" onclick={batchDelete} title="删除选中项（需二次确认）">删除</button>
  </div>
{/if}

<style>
  .convert-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 8px 18px 0;
    padding: 8px 12px;
    background: var(--color-primary-soft);
    border: 1px solid var(--color-primary);
    border-radius: 8px;
    font-size: 13px;
  }
  .convert-bar span {
    flex: 1;
  }
  .convert-bar .ignore {
    border-color: transparent;
    background: transparent;
    color: var(--color-text-dim);
  }
  .todo-list {
    list-style: none;
    margin: 0;
    padding: 6px 0;
    flex: 1;
    overflow-y: auto;
  }
  .todo-row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 6px 18px;
    border-bottom: 1px solid var(--color-border);
  }
  .todo-row:hover {
    background: var(--color-hover);
  }
  .todo-row.dragging {
    opacity: 0.4;
  }
  .todo-row.can-drag {
    cursor: grab;
    touch-action: none;
    user-select: none;
  }
  .todo-row.can-drag.dragging {
    cursor: grabbing;
  }
  .todo-row.drop-above {
    box-shadow: inset 0 2px 0 0 var(--color-primary);
  }
  .todo-row.drop-below {
    box-shadow: inset 0 -2px 0 0 var(--color-primary);
  }
  .title {
    min-width: 0;
    cursor: text;
  }
  .done .title {
    text-decoration: line-through;
    color: var(--color-text-dim);
  }
  .tags {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }
  .pill {
    font-size: 11px;
    line-height: 1.4;
    padding: 0 7px;
    border-radius: 10px;
    border: 1px solid var(--color-primary);
    color: var(--color-primary);
    background: var(--color-primary-soft);
  }
  .tag-add {
    font-size: 12px;
    line-height: 1;
    padding: 1px 6px;
    border-radius: 8px;
    color: var(--color-text-dim);
  }
  .tag-edit {
    width: 130px;
    font-size: 12px;
    padding: 2px 6px;
  }
  .spacer {
    flex: 1;
  }
  .inline-edit {
    flex: 1;
  }
  .color-wrap {
    position: relative;
    display: inline-flex;
  }
  .color-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 1px solid var(--color-border);
    padding: 0;
    background: transparent;
  }
  .color-pop {
    position: absolute;
    top: 16px;
    left: 0;
    z-index: 10;
    display: flex;
    gap: 3px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 4px;
    box-shadow: var(--shadow);
  }
  .swatch {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 1px solid var(--color-border);
    padding: 0;
  }
  .swatch.none {
    background: var(--color-bg);
    font-size: 10px;
    line-height: 1;
  }
  .priority {
    font-size: 12px;
    padding: 2px 4px;
    border-radius: 5px;
  }
  .priority.p-high {
    color: #dc2626;
    font-weight: 600;
  }
  .mini {
    font-size: 12px;
    padding: 2px 8px;
  }
  .fold {
    width: 18px;
    border: none;
    background: transparent;
    color: var(--color-text-dim);
    padding: 0;
    font-size: 11px;
  }
  .fold:hover { color: var(--color-primary); }
  .fold-placeholder { width: 18px; flex-shrink: 0; }
  .add-sub {
    font-size: 11px;
    padding: 1px 6px;
    color: var(--color-text-dim);
    border-color: transparent;
    background: transparent;
    visibility: hidden;
  }
  .todo-row:hover .add-sub { visibility: visible; }
  .add-sub:hover { color: var(--color-primary); }
  .clock {
    font-size: 12px;
    opacity: 0.7;
  }
  .assign-btn {
    font-size: 12px;
    padding: 2px 8px;
    color: var(--color-text-dim);
  }
  .assign-pop {
    position: absolute;
    right: 40px;
    z-index: 20;
    display: flex;
    flex-direction: column;
    gap: 4px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 10px;
    box-shadow: var(--shadow);
    min-width: 160px;
  }
  .assign-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    cursor: pointer;
  }
  .assign-empty {
    font-size: 12px;
    color: var(--color-text-dim);
  }
  .assign-save {
    margin-top: 4px;
    font-size: 12px;
  }
  .del {
    border: none;
    background: transparent;
    color: var(--color-text-dim);
    padding: 2px 8px;
  }
  .detail-btn {
    border: none;
    background: transparent;
    color: var(--color-text-dim);
    padding: 2px 6px;
    font-size: 14px;
  }
  .detail-btn.on {
    color: var(--color-primary);
    font-weight: 700;
  }
  .del:hover {
    color: #dc2626;
  }
  .empty {
    padding: 36px 18px;
    text-align: center;
    color: var(--color-text-dim);
  }
  /* ---- 多选批量 ---- */
  .batch-toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 18px 0;
    font-size: 12px;
  }
  .batch-toolbar .mini {
    font-size: 12px;
    padding: 2px 8px;
  }
  .sel-count {
    color: var(--color-text-dim);
  }
  .sel-box {
    flex-shrink: 0;
  }
  .todo-row.sel {
    background: var(--color-primary-soft);
  }
  .batch-bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    padding: 8px 18px;
    border-top: 1px solid var(--color-border);
    background: var(--color-surface);
    font-size: 12px;
  }
  .batch-bar .mini {
    font-size: 12px;
    padding: 2px 8px;
  }
  .batch-bar .danger {
    color: #dc2626;
  }
  .batch-date {
    font-size: 12px;
    padding: 1px 4px;
  }
  .sep {
    width: 1px;
    height: 16px;
    background: var(--color-border);
    margin: 0 2px;
  }
  .colors {
    display: inline-flex;
    gap: 4px;
    align-items: center;
  }
  .batch-bar .swatch {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1px solid var(--color-border);
    padding: 0;
  }
</style>
