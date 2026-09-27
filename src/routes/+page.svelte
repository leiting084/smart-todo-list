<script lang="ts">
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Today from "$lib/views/Today.svelte";
  import Inbox from "$lib/views/Inbox.svelte";
  import Category from "$lib/views/Category.svelte";
  import TaggedTodos from "$lib/views/TaggedTodos.svelte";
  import Projects from "$lib/views/Projects.svelte";
  import People from "$lib/views/People.svelte";
  import Goals from "$lib/views/Goals.svelte";
  import TodoDetail from "$lib/components/TodoDetail.svelte";
  import Dashboard from "$lib/views/Dashboard.svelte";
  import Settings from "$lib/views/Settings.svelte";
  import Overview from "$lib/views/Overview.svelte";
  import Memos from "$lib/views/Memos.svelte";
  import Notes from "$lib/views/Notes.svelte";
  import { NAV_ITEMS, ui } from "$lib/stores.svelte";

  const current = $derived(NAV_ITEMS.find((i) => i.id === ui.view));
  const paneKey = $derived(
    ui.activeTag
      ? `tag:${ui.activeTag}`
      : ui.view === "today" && ui.overdueMode
        ? "today:overdue"
        : ui.view,
  );
  const headerTitle = $derived(
    ui.activeTag
      ? `#${ui.activeTag}`
      : ui.view === "today" && ui.overdueMode
        ? "逾期"
        : current?.label,
  );
</script>

<svelte:head>
  <title>智能待办清单</title>
</svelte:head>

<div class="app">
  <Sidebar />

  <!-- 中：列表区 -->
  <section class="list-pane">
    <header class="pane-header">{headerTitle}</header>
    <div class="list-body">
      {#key paneKey}
        {#if ui.activeTag}
          <TaggedTodos />
        {:else if ui.view === "dashboard"}
          <Dashboard />
        {:else if ui.view === "today"}
          <Today />
        {:else if ui.view === "inbox"}
          <Inbox />
        {:else if ui.view === "all"}
          <Category />
        {:else if ui.view === "work"}
          <Category category="work" />
        {:else if ui.view === "life"}
          <Category category="life" />
        {:else if ui.view === "projects"}
          <Projects />
        {:else if ui.view === "people"}
          <People />
        {:else if ui.view === "goals"}
          <Goals />
        {:else if ui.view === "settings"}
          <Settings />
        {:else if ui.view === "memos"}
          <Memos />
        {:else if ui.view === "notes"}
          <Notes />
        {:else if ui.view === "overview"}
          <Overview />
        {:else}
          <div class="placeholder">
            <p>「{current?.label}」列表区</p>
            <p class="hint">视图骨架已就位，内容在后续任务实现。</p>
          </div>
        {/if}
      {/key}
    </div>
  </section>

  <!-- 右：详情区（窄屏折叠，SPEC §8.1；T1.4） -->
  <section class="detail-pane">
    <TodoDetail />
  </section>
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: 190px 1fr 300px;
    height: 100vh;
  }

  .list-pane,
  .detail-pane {
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--color-bg);
  }
  .list-pane {
    height: 100vh;
  }
  .list-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .detail-pane {
    border-left: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  .pane-header {
    padding: 14px 18px;
    font-size: 16px;
    font-weight: 600;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  .placeholder {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: var(--color-text-dim);
    text-align: center;
    padding: 20px;
  }
  .placeholder p {
    margin: 0;
  }
  .hint {
    font-size: 12px;
    opacity: 0.75;
  }

  @media (max-width: 900px) {
    .app {
      grid-template-columns: 190px 1fr;
    }
    .detail-pane {
      display: none;
    }
  }
</style>
