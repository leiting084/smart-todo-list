<script lang="ts">
  // 快速添加分类的分段切换：绑定共享的 ui.quickAddCategory，点一下即记住（持久化）。
  import { ui } from "../stores.svelte";
  import { setQuickAddCategory } from "../quickAdd";

  const OPTS = [
    { value: "work", label: "工作" },
    { value: "life", label: "生活" },
  ] as const;
</script>

<div class="cat-toggle" role="group" aria-label="新待办分类">
  {#each OPTS as o (o.value)}
    <button
      type="button"
      class="seg"
      class:active={ui.quickAddCategory === o.value}
      aria-pressed={ui.quickAddCategory === o.value}
      title={`新添加的待办归到「${o.label}」`}
      onclick={() => void setQuickAddCategory(o.value)}
    >{o.label}</button>
  {/each}
</div>

<style>
  .cat-toggle {
    display: inline-flex;
    flex: none;
    border: 1px solid var(--color-border);
    border-radius: 7px;
    overflow: hidden;
    height: 30px;
  }
  .seg {
    border: none;
    background: var(--color-surface);
    color: var(--color-text-dim);
    padding: 0 12px;
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
  }
  .seg + .seg {
    border-left: 1px solid var(--color-border);
  }
  .seg:hover {
    background: var(--color-hover);
  }
  .seg.active {
    background: var(--color-primary);
    color: var(--color-primary-fg);
    font-weight: 600;
  }
</style>
