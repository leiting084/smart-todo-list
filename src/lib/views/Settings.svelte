<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    api,
    type ImportReport,
    type DataDirInfo,
    type MiniSettings,
    type AiConfigView,
    type AiProvider,
  } from "../api";
  import { setFollowSystem as setFollowTheme } from "../theme";

  let report = $state<ImportReport | null>(null);
  let importing = $state(false);
  let importError = $state("");
  let fileInput = $state<HTMLInputElement | null>(null);
  let dataDir = $state("");
  let dirInfo = $state<DataDirInfo | null>(null);
  let dirBusy = $state(false);
  let dirMsg = $state("");
  let scShow = $state("ctrl+alt+t");
  let scMini = $state("ctrl+alt+n");
  let mini = $state<MiniSettings | null>(null);
  let aiView = $state<AiConfigView | null>(null);
  let aiProviders = $state<AiProvider[]>([]);
  let aiCurrent = $state<string | null>(null);
  let aiPrompt = $state("");
  let aiMsg = $state("");
  let aiModels = $state<string[]>([]);
  let updateUrlInput = $state("");
  let updateMsg = $state("");
  let remindersOn = $state(true);
  let followSystemTheme = $state(false);
  let clipImporting = $state(false);
  let clipRows = $state<string[]>([]);
  let clipDate = $state("");
  let clipCategory = $state<"life" | "work">("life");
  let clipResult = $state<{ created: number; failed: string[] } | null>(null);

  async function setFollowSystem(on: boolean) {
    try {
      await setFollowTheme(on);
      followSystemTheme = on;
    } catch (e) {
      alert(`设置失败：${e}`);
    }
  }
  void api.settingsGet("theme").then((v) => (followSystemTheme = v === "system")).catch(() => {});

  /** T3.9：读剪贴板 → 按行解析 → 可编辑列表 */
  async function clipboardImport() {
    try {
      const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
      clipImporting = true;
      const text = await readText();
      clipResult = null;
      const rows = text
        .split(/\r?\n/)
        .map((l) => l.replace(/^\s*(?:[-*]\s+|\d+\.\s+|•\s+)/, "").trim())
        .filter((l) => l.length > 0);
      if (rows.length === 0) {
        alert("剪贴板没有可解析的行。");
      }
      clipRows = rows;
    } catch (e) {
      alert(`读取剪贴板失败：${e}`);
    } finally {
      clipImporting = false;
    }
  }
  async function clipSubmit() {
    const items = clipRows
      .filter((t) => t.trim())
      .map((title) => ({
        title: title.trim(),
        date: clipDate ? Number(clipDate.replaceAll("-", "")) : null,
        category: clipCategory,
        priority: "medium" as const,
      }));
    const [created, failed] = await api.todosBulkCreate(items);
    clipResult = { created, failed: failed ?? [] };
  }

  let exportOnlyActive = $state(true);
  let exportMsg = $state("");

  async function exportAs(kind: "md" | "csv" | "html") {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const ext = { md: "md", csv: "csv", html: "html" }[kind];
      const path = await save({
        filters: [{ name: kind.toUpperCase(), extensions: [ext] }],
        defaultPath: `待办导出.${ext}`,
      });
      if (!path) return;
      const filter = { includeDone: exportOnlyActive ? false : null };
      const [, n] =
        kind === "md" ? await api.exportMarkdown(String(path), filter) :
        kind === "csv" ? await api.exportCsv(String(path), filter) : await api.exportHtml(String(path), filter);
      exportMsg = `已导出 ${n} 条到 ${path}`;
      try {
        const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
        await revealItemInDir(String(path));
      } catch {
        /* 打开目录失败可忽略 */
      }
    } catch (e) {
      alert(`导出失败：${e}`);
    }
  }


  void api.appDataDir().then((d) => (dataDir = d)).catch(() => {});
  void loadDirInfo();
  void loadShortcuts();
  void loadMini();
  void loadAi();
  void loadReminders();

  async function loadReminders() {
    try {
      remindersOn = await invoke<boolean>("reminders_get_enabled");
    } catch {
      /* 浏览器环境 */
    }
  }
  async function setReminders(on: boolean) {
    try {
      await invoke("reminders_set_enabled", { enabled: on });
      remindersOn = on;
    } catch (e) {
      alert(`设置失败：${e}`);
    }
  }

  async function loadShortcuts() {
    try {
      const obj = await invoke<{ showMain: string; toggleMini: string }>("get_shortcuts");
      scShow = obj.showMain;
      scMini = obj.toggleMini;
    } catch {
      /* 浏览器环境 */
    }
  }

  async function loadMini() {
    try {
      mini = await api.miniGetSettings();
    } catch {
      mini = null;
    }
  }

  async function saveMiniPin() {
    if (!mini) return;
    try {
      mini = await api.miniSetPinMode(mini.pin_mode as "top" | "bottom" | "none");
    } catch {
      /* 保留界面值 */
    }
  }

  async function saveMiniOpacity() {
    if (!mini) return;
    try {
      mini = await api.miniSetOpacity(mini.opacity);
    } catch {
      /* 保留界面值 */
    }
  }

  /** 复选框：项目约定用 onclick（onchange 委托在本环境不可靠） */
  async function toggleMiniEdgeHide() {
    if (!mini) return;
    try {
      mini = await api.miniSetEdgeHide(!mini.edge_hide);
    } catch {
      /* 保留界面值 */
    }
  }

  // ---- AI 周报（F26/T5.1）----
  const AI_PRESETS = [
    { name: "DeepSeek", base_url: "https://api.deepseek.com", model: "deepseek-chat" },
    { name: "智谱 GLM", base_url: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash" },
    { name: "Moonshot", base_url: "https://api.moonshot.cn/v1", model: "moonshot-v1-8k" },
    { name: "本地 Ollama", base_url: "http://localhost:11434/v1", model: "qwen2.5:7b" },
  ];

  function newId() {
    return `p${Date.now()}${Math.floor(Math.random() * 1000)}`;
  }

  async function loadAi() {
    try {
      aiView = await api.aiGetConfig();
      // 后端视图不含明文 key，本地副本的 key 留空（提交时留空 = 保留原值）
      aiProviders = aiView.providers.map((p) => ({
        id: p.id,
        name: p.name,
        base_url: p.base_url,
        model: p.model,
        api_key: "",
      }));
      aiCurrent = aiView.current_provider_id;
      aiPrompt = aiView.system_prompt;
    } catch {
      aiView = null;
    }
  }

  function addProvider() {
    aiProviders = [
      ...aiProviders,
      { id: newId(), name: "自定义", base_url: "", model: "", api_key: "" },
    ];
  }

  function removeProvider(id: string) {
    aiProviders = aiProviders.filter((p) => p.id !== id);
    if (aiCurrent === id) aiCurrent = null;
  }

  async function saveProviders() {
    aiMsg = "";
    try {
      aiView = await api.aiSaveProviders(aiProviders, aiCurrent);
      aiMsg = "已保存";
    } catch (e) {
      aiMsg = `保存失败：${e}`;
    }
  }

  async function testAiConnection() {
    aiMsg = "";
    aiModels = [];
    await saveProviders();
    try {
      aiModels = await api.aiTestConnection(aiCurrent ?? "");
      aiMsg = `连接成功，取到 ${aiModels.length} 个模型`;
    } catch (e) {
      aiMsg = `连接失败：${e}`;
    }
  }

  async function saveAiPrompt() {
    aiMsg = "";
    try {
      aiView = await api.aiSetPrompt(aiPrompt);
      aiMsg = "prompt 已保存";
    } catch (e) {
      aiMsg = `保存失败：${e}`;
    }
  }

  async function resetAiPrompt() {
    aiMsg = "";
    try {
      aiView = await api.aiSetPrompt("");
      aiPrompt = aiView.system_prompt;
      aiMsg = "已恢复默认 prompt";
    } catch (e) {
      aiMsg = `恢复失败：${e}`;
    }
  }

  // ---- 更新检查（F27/T5.2）----
  async function saveUpdateUrl() {
    updateMsg = "";
    try {
      const v = await api.setUpdateUrl(updateUrlInput);
      updateMsg = v ? `更新源已设为 ${v}` : "已清除更新源";
    } catch (e) {
      updateMsg = `保存失败：${e}`;
    }
  }

  async function doCheckUpdate() {
    updateMsg = "检查中…";
    try {
      const info = await api.checkUpdate();
      updateMsg = info.hasUpdate
        ? `有新版本 ${info.latest}（当前 ${info.current}）`
        : `已是最新（${info.current}）`;
    } catch (e) {
      updateMsg = `${e}`;
    }
  }

  async function saveShortcuts() {
    try {
      await api.settingsSet("shortcut_show_main", scShow.trim().toLowerCase());
      await api.settingsSet("shortcut_toggle_mini", scMini.trim().toLowerCase());
      alert("已保存，重启应用后生效。");
    } catch (e) {
      alert(`保存失败：${e}`);
    }
  }

  /** T4.3：拉取数据目录信息 */
  async function loadDirInfo() {
    try {
      dirInfo = await api.getDataDirInfo();
      dataDir = dirInfo.dataDir;
    } catch {
      /* 浏览器环境 */
    }
  }

  /** T4.3：迁移数据到自选目录，成功后重启（进程内路径是最早初始化的，只能靠重启生效） */
  async function switchDataDir(target: string | null) {
    if (dirBusy) return;
    dirBusy = true;
    dirMsg = "";
    try {
      const info = await api.setDataDir(target);
      dirInfo = info;
      dirMsg = "已切换，正在重启…";
      await api.relaunchApp();
    } catch (e) {
      alert(`切换数据目录失败：${e}`);
    } finally {
      dirBusy = false;
    }
  }

  async function chooseDataDir() {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ directory: true, multiple: false });
      if (!picked) return;
      const target = String(picked);
      if (!confirm(`把数据迁移到：\n${target}\n\n完成后会自动重启应用。继续？`)) return;
      await switchDataDir(target);
    } catch (e) {
      alert(`选择目录失败：${e}`);
    }
  }

  async function resetDataDir() {
    if (!confirm("恢复为绿色默认目录（App 同级的 data/），原自定义目录的数据会保留不会删除。继续？")) return;
    await switchDataDir(null);
  }

  async function onFile(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    importing = true;
    importError = "";
    report = null;
    try {
      const content = await file.text();
      report = await api.importLegacyJson(content);
    } catch (err) {
      importError = String(err);
    } finally {
      importing = false;
      input.value = ""; // 允许重复选同一文件
    }
  }
</script>

<div class="settings">
  <h2>设置</h2>

  <section class="block">
    <h3>外观</h3>
    <label class="switch-row">
      <input type="checkbox" checked={followSystemTheme} onclick={(e) => void setFollowSystem((e.target as HTMLInputElement).checked)} />
      跟随系统深浅色
    </label>
  </section>

  <section class="block">
    <h3>剪贴板快速导入（T3.9）</h3>
    <p class="hint">把一段多行文本复制到剪贴板，点下方按钮：按行拆分为待办（自动去掉 `- ` `* ` `1.` 列表符号与空行），可编辑后选日期/分类一次入库。</p>
    <button onclick={clipboardImport} disabled={clipImporting}>{clipImporting ? "解析中…" : "从剪贴板解析…"}</button>

    {#if clipRows.length}
      <div class="clip-edit">
        <div class="clip-opts">
          <label>日期 <input type="date" bind:value={clipDate} /></label>
          <label>分类
            <select bind:value={clipCategory}>
              <option value="life">生活</option>
              <option value="work">工作</option>
            </select>
          </label>
          <button class="primary" onclick={clipSubmit} disabled={clipRows.length === 0}>创建 {clipRows.length} 条</button>
        </div>
        {#each clipRows as row, i (i)}
          <div class="clip-row">
            <span class="idx">{i + 1}</span>
            <input type="text" bind:value={clipRows[i]} />
          </div>
        {/each}
      </div>
    {/if}
    {#if clipResult}
      <p class="ok">创建 {clipResult.created} 条{#if clipResult.failed.length}，失败 {clipResult.failed.length}（{clipResult.failed.slice(0, 3).join("；")}）{/if}</p>
    {/if}
  </section>

  <section class="block">
    <h3>数据目录（F9 / F22）</h3>
    <p class="mono">{dirInfo ? dirInfo.dataDir : dataDir || "…"}</p>
    <p class="hint">
      {#if dirInfo?.isCustom}
        当前为自定义目录（配置写在应用根目录的 app-config.json）。把它放进网盘/同步盘即可多机同步。
      {:else}
        当前为绿色默认目录（App 同级的 data/）。整个文件夹拷走即可迁移。
      {/if}
    </p>
    <div class="clip-opts">
      <button disabled={dirBusy} onclick={chooseDataDir}>更改到…</button>
      <button disabled={dirBusy || !dirInfo?.isCustom} onclick={resetDataDir}>恢复默认目录</button>
    </div>
    {#if dirMsg}
      <p class="ok">{dirMsg}</p>
    {/if}
    <p class="hint">
      切换会把现有 data/ 迁移到目标目录并重启应用；若目标目录里已有 todolist.db，则直接采用它的数据（不覆盖）。原目录数据不会被删除。
    </p>
  </section>

  <section class="block">
    <h3>数据导入</h3>
    <p class="hint">
      选择旧版「智能待办清单」的 <code>todolist-data.json</code>：任务（含项目/人员分配/协作完成/标签）、
      项目、人员、预设标签、长期目标将全量导入。导入前自动备份数据库；同一文件重复导入会自动跳过。
    </p>
    <input
      type="file"
      accept=".json,application/json"
      style="display:none"
      bind:this={fileInput}
      onchange={onFile}
    />
    <button class="primary" disabled={importing} onclick={() => fileInput?.click()}>
      {importing ? "导入中…" : "选择 JSON 文件导入…"}
    </button>
    {#if importError}
      <p class="err">{importError}</p>
    {/if}
    {#if report}
      <div class="report">
        {#if report.alreadyImported}
          <p class="ok">该文件已导入过，已自动跳过（如需重导请先删除 settings 中的导入指纹或使用其他文件副本）。</p>
        {:else}
          <p class="ok">
            导入完成：任务 {report.tasks} · 项目 {report.projects} · 人员 {report.people} ·
            标签 {report.tags} · 目标 {report.goals}
          </p>
        {/if}
        {#if report.skipped.length > 0}
          <p class="warn">跳过 {report.skipped.length} 项：</p>
          <ul>{#each report.skipped as s}<li>{s}</li>{/each}</ul>
        {/if}
        {#each report.notes as n}<p class="hint">{n}</p>{/each}
      </div>
    {/if}
  </section>

  <section class="block">
    <h3>提醒通知</h3>
    <label class="switch-row">
      <input type="checkbox" checked={remindersOn} onclick={(e) => void setReminders((e.target as HTMLInputElement).checked)} />
      启用全局提醒（到点弹系统通知）
    </label>
    <p class="hint">进程未运行期间到期的提醒不补弹；同一待办同一天只提醒一次。</p>
  </section>

  <section class="block">
    <h3>全局快捷键（改后重启生效）</h3>
    <div class="sc-row">
      <span class="sc-label">呼出主窗</span>
      <input type="text" bind:value={scShow} placeholder="如 ctrl+alt+t" />
    </div>
    <div class="sc-row">
      <span class="sc-label">开关浮窗</span>
      <input type="text" bind:value={scMini} placeholder="如 ctrl+alt+n" />
    </div>
    <button onclick={saveShortcuts}>保存快捷键</button>
    <p class="hint">格式：mod 键小写加号连接（ctrl+alt+t）。与其他程序冲突时该键将不注册。</p>
  </section>

  <section class="block">
    <h3>迷你浮窗（F12）</h3>
    {#if mini}
      <div class="sc-row">
        <span class="sc-label">窗口层级</span>
        <select bind:value={mini.pin_mode} onchange={saveMiniPin}>
          <option value="top">置顶（默认）</option>
          <option value="bottom">置底 · 钉桌面</option>
          <option value="none">普通窗口</option>
        </select>
      </div>
      <div class="sc-row">
        <span class="sc-label">透明度</span>
        <input
          type="range"
          min="0.35"
          max="1"
          step="0.05"
          bind:value={mini.opacity}
          onchange={saveMiniOpacity}
        />
        <span class="mono">{Math.round(mini.opacity * 100)}%</span>
      </div>
      <div class="sc-row">
        <span class="sc-label">贴边隐藏</span>
        <input type="checkbox" checked={mini.edge_hide} onclick={toggleMiniEdgeHide} />
      </div>
      <p class="hint">
        置底可把浮窗钉在桌面（不挡其他窗口）。贴边隐藏开启后，鼠标移出浮窗即缩成边条贴在最近的屏幕边缘，鼠标移入展开。三项改动即时生效，无需重启。
      </p>
    {:else}
      <p class="hint">浮窗设置不可用（后端未就绪）。</p>
    {/if}
  </section>


  <section class="block">
    <h3>数据导出（T4.1：Markdown / CSV(带BOM) / HTML 可打印）</h3>
    <p class="hint">选择格式与保存位置；CSV 为 UTF-8 带 BOM（Excel 打开不乱码）。可选仅导未完成。</p>
    <div class="clip-opts">
      <label>仅未完成 <input type="checkbox" bind:checked={exportOnlyActive} /></label>
      <button onclick={() => exportAs("md")}>导出 Markdown…</button>
      <button onclick={() => exportAs("csv")}>导出 CSV…</button>
      <button onclick={() => exportAs("html")}>导出 HTML…</button>
    </div>
    {#if exportMsg}
      <p class="ok">{exportMsg}</p>
    {/if}
  </section>

  <section class="block">
    <h3>AI 周报（F26 · 自配接口）</h3>
    <p class="hint">
      只做自配轨：接口地址与 Key 由你自备，请求直连你填的 base_url，**不经任何第三方中转**。
      Key 只存在本机 <code>app-config.json</code>，不进数据库、不进备份包、不进导出文件。
    </p>
    {#if aiView}
      {#each aiProviders as p (p.id)}
        <div class="prov">
          <div class="prov-row">
            <input class="pname" placeholder="名称" bind:value={p.name} />
            <select
              onchange={(e) => {
                const idx = Number((e.currentTarget as HTMLSelectElement).value);
                if (idx >= 0) {
                  const s = AI_PRESETS[idx];
                  p.name = s.name;
                  p.base_url = s.base_url;
                  p.model = s.model;
                }
              }}
            >
              <option value={-1}>预设服务商…</option>
              {#each AI_PRESETS as s, i}
                <option value={i}>{s.name}</option>
              {/each}
            </select>
            <label class="cur">
              <input
                type="radio"
                name="ai-current"
                checked={aiCurrent === p.id}
                onclick={() => (aiCurrent = p.id)}
              />
              当前
            </label>
            <button class="del" onclick={() => removeProvider(p.id)}>删除</button>
          </div>
          <div class="prov-row">
            <input class="purl" placeholder="base_url，如 https://api.deepseek.com" bind:value={p.base_url} />
            <input class="pmodel" placeholder="模型名" bind:value={p.model} />
          </div>
          <div class="prov-row">
            <input
              class="pkey"
              type="password"
              placeholder="API Key（留空 = 保留已保存的值）"
              bind:value={p.api_key}
            />
            <span class="khint">{aiView.providers.find((v) => v.id === p.id)?.key_hint ?? ""}</span>
          </div>
        </div>
      {/each}
      <div class="btn-row">
        <button onclick={addProvider}>+ 新增服务商</button>
        <button onclick={saveProviders}>保存</button>
        <button onclick={testAiConnection}>测试连接</button>
      </div>
      {#if aiModels.length}
        <p class="hint">可用模型（点击填入「当前」服务商）：</p>
        <div class="models">
          {#each aiModels as m}
            <button class="mdl" onclick={() => {
              const t = aiProviders.find((p) => p.id === aiCurrent);
              if (t) t.model = m;
            }}>{m}</button>
          {/each}
        </div>
      {/if}
      <p class="sub">System prompt（留空并保存 = 恢复内置默认）</p>
      <textarea rows="6" bind:value={aiPrompt}></textarea>
      <div class="btn-row">
        <button onclick={saveAiPrompt}>保存 prompt</button>
        <button onclick={resetAiPrompt}>恢复默认</button>
      </div>
      {#if aiMsg}<p class="msg">{aiMsg}</p>{/if}
    {:else}
      <p class="hint">AI 配置不可用（后端未就绪）。</p>
    {/if}
  </section>

  <section class="block">
    <h3>更新检查（F27）</h3>
    <p class="hint">
      绿色便携版**不做自动更新**（不引 updater、不做签名）：有新版本时下载新包覆盖 exe 即可，
      <code>data/</code> 在同级目录不受影响。这里只做手动检查——填一个返回
      <code>{"{"}"version":"0.3.0","url":"…"{"}"}</code> 的 JSON 地址即可。
    </p>
    <input placeholder="更新源 JSON 地址（留空 = 不检查）" bind:value={updateUrlInput} />
    <div class="btn-row">
      <button onclick={saveUpdateUrl}>保存地址</button>
      <button onclick={doCheckUpdate}>检查更新</button>
    </div>
    {#if updateMsg}<p class="msg">{updateMsg}</p>{/if}
  </section>

  <section class="block">
    <h3>关于</h3>
    <p class="hint">智能待办清单 v0.2.0 · Rust + Tauri 2 · 数据本地存储，默认完全离线。关闭主窗=驻留托盘，Esc 同效。</p>
  </section>
</div>

<style>
  .settings { padding: 14px 18px; overflow-y: auto; flex: 1; display: flex; flex-direction: column; gap: 16px; }
  h2 { margin: 0; font-size: 18px; }
  h3 { margin: 0 0 6px; font-size: 13px; color: var(--color-text-dim); }
  .block { background: var(--color-surface); border: 1px solid var(--color-border); border-radius: 8px; padding: 12px 14px; }
  .mono { font-family: Consolas, monospace; font-size: 12px; word-break: break-all; margin: 0 0 4px; }
  .hint { font-size: 12px; color: var(--color-text-dim); margin: 4px 0; }
  code { background: var(--color-bg); border-radius: 4px; padding: 0 4px; }
  .err { color: #dc2626; font-size: 13px; }
  .ok { color: var(--color-primary); font-size: 13px; }
  .warn { color: #f59e0b; font-size: 13px; margin-bottom: 0; }
  .report ul { margin: 4px 0; padding-left: 20px; font-size: 12px; color: var(--color-text-dim); }
</style>
