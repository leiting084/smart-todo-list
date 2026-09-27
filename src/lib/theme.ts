// 明/暗/跟随系统主题：应用到 <html data-theme> 并持久化到 settings 表。
import { api, SETTING_KEYS } from "./api";
import { ui, type Theme } from "./stores.svelte";

let mql: MediaQueryList | undefined;

function apply(theme: Exclude<Theme, "system">) {
  ui.theme = theme;
  document.documentElement.dataset.theme = theme;
}

function systemPrefersDark() {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function attachSystemListener() {
  if (mql) return;
  mql = window.matchMedia("(prefers-color-scheme: dark)");
  mql.addEventListener("change", applySystem);
}
function detachSystemListener() {
  mql?.removeEventListener("change", applySystem);
  mql = undefined;
}
function applySystem() {
  apply(systemPrefersDark() ? "dark" : "light");
}

/** 启动：优先 settings 恢复；'system' 或未设置 → 跟随系统并监听实时变化。 */
export async function initTheme() {
  let saved: string | null = null;
  try {
    saved = await api.settingsGet(SETTING_KEYS.theme);
  } catch {
    /* 浏览器 */
  }
  if (saved === "light" || saved === "dark") {
    apply(saved);
  } else {
    ui.theme = "system";
    attachSystemListener();
    applySystem();
  }
}

/** 设置页：开启/关闭"跟随系统"。 */
export async function setFollowSystem(on: boolean) {
  if (on) {
    ui.theme = "system";
    attachSystemListener();
    applySystem();
    await api.settingsSet(SETTING_KEYS.theme, "system");
  } else {
    detachSystemListener();
    apply(systemPrefersDark() ? "dark" : "light");
    await api.settingsSet(SETTING_KEYS.theme, ui.theme);
  }
}

/** 侧栏切换：仅在 light/dark 间切换（跟随系统时基于当前实际值）。 */
export async function toggleTheme() {
  detachSystemListener();
  const current = ui.theme === "system" ? (systemPrefersDark() ? "dark" : "light") : ui.theme;
  const next: "light" | "dark" = current === "dark" ? "light" : "dark";
  apply(next);
  try {
    await api.settingsSet(SETTING_KEYS.theme, next);
  } catch (e) {
    console.error("主题持久化失败", e);
  }
}