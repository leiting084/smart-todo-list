<script lang="ts">
  import { onMount } from "svelte";
  import TodoList from "../components/TodoList.svelte";
  import { ui } from "../stores.svelte";
  import { loadTodos } from "../todos.svelte";

  onMount(() => {
    if (ui.activeTag) loadTodos({ tag: ui.activeTag });
  });
</script>

<div class="pane">
  <TodoList
    reorderable={false}
    emptyText={`没有挂「${ui.activeTag}」标签的待办。`}
    onReorder={() => {}}
  />
</div>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
</style>
