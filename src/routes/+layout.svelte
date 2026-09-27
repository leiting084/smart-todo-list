<script lang="ts">
  import { onDestroy } from "svelte";
  import "../app.css";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { initTheme } from "$lib/theme";
  import { initQuickAddCategory } from "$lib/quickAdd";
  import { invoke } from "@tauri-apps/api/core";
  import { loadTodos, todosState } from "$lib/todos.svelte";

  let { children } = $props();

  let winUnlistens: (() => void)[] = [];

  /** T3.3：Esc 隐藏主窗（驻留托盘；托盘/快捷键呼出） */
  async function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      getCurrentWindow().hide();
    } catch {
      /* 浏览器环境 */
    }
  }

  onMount(async () => {
    await initTheme();
    await initQuickAddCategory();
    // T0.2：窗口标题显示当前数据路径（Tauri 同步 document.title）
    try {
      const dir = await api.appDataDir();
      document.title = `智能待办清单 — data: ${dir}`;
    } catch {
      // 普通浏览器打开 dev server 时无 IPC
    }
    // T3.2：浮窗改动跨窗刷新（仅主窗加载列表时响应）
    try {
      const { listen } = await import("@tauri-apps/api/event");
      await listen("todos-changed", () => {
        // 当前视图含待办列表时整体刷新（filter 由各视图自持——简单方案：触发当前视图 remount 不可行，直接拉全量）
        void loadTodos({}).catch(() => {});
        void todosState;
      });
    } catch {
      /* 无事件 API（浏览器） */
    }
    // T3.8：主窗移动/缩放防抖保存位置尺寸（多显示器拔掉由后端 close 回主屏）
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      let timer: ReturnType<typeof setTimeout> | undefined;
      const save = async () => {
        clearTimeout(timer);
        timer = setTimeout(async () => {
          try {
            const pos = await win.outerPosition();
            const size = await win.outerSize();
            await invoke("window_save_bounds", {
              label: "main",
              x: pos.x,
              y: pos.y,
              width: size.width,
              height: size.height,
            });
          } catch {
            /* 窗口关闭中 */
          }
        }, 800);
      };
      const u1 = await win.onMoved(() => void save());
      const u2 = await win.onResized(() => void save());
      winUnlistens.push(u1, u2);
    } catch {
      /* 浏览器 */
    }
  });

  onDestroy(() => {
    winUnlistens.forEach((u) => u());
  });
</script>

<svelte:window onkeydown={onKeydown} />

{@render children()}
