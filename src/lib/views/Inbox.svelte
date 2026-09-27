<script lang="ts">
  import { onMount } from "svelte";
  import TodoList from "../components/TodoList.svelte";
  import { loadToday, loadTodos, persistOrder } from "../todos.svelte";

  onMount(async () => {
    await loadToday();
    await loadTodos({ inbox: true });
  });
</script>

<div class="pane">
  <TodoList
    inboxMode
    emptyText="收集箱是空的。有想法先丢这里，再安排日期。"
    onReorder={persistOrder}
    onRefresh={() => loadTodos({ inbox: true })}
  />
</div>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
</style>
