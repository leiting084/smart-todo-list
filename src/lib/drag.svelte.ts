// 指针拖拽（pointer-events）共享状态。
// 背景：WebView2 里 HTML5 原生拖拽不可靠（真鼠标也拖不动，见 2026-09-27 排查），
// 故换序与"拖到日期改期"改用 pointerdown/move/up 自实现，绕开原生 OLE 拖放。
// 文件用 .svelte.ts 才能在模块顶层使用 runes。

export interface DragState {
  /** 正在被指针拖拽的待办 id；null = 无拖拽 */
  draggingId: string | null;
  /** 指针当前悬停的改期目标日期（YYYYMMDD）；0 = 未悬停到日期栏 */
  hoverDate: number;
}

export const drag = $state<DragState>({
  draggingId: null,
  hoverDate: 0,
});
