#!/usr/bin/env node
/**
 * 绿色便携版一键构建 —— 本项目**唯一**的打包入口。
 *
 *   pnpm build:green            # 前端未变则跳过 vite build
 *   pnpm build:green -- --force # 强制重编前端
 *
 * 背景见 .claude/rules/packaging-green-only.md：
 * 唯一交付物是 `dist/智能待办清单-绿色版/`（exe + 使用说明.txt，首次运行自建 data/），
 * 禁止产出 NSIS/MSI 安装器。为此 tauri.conf.json 里 `bundle.active` 恒为 false，
 * 本脚本末尾还会断言 dist/ 内没有安装器。
 *
 * ⚠️ vite build 在本机有卡死问题：**卡在 "building ssr environment for production..."**，
 * 表现为 CPU 归零、无任何产物写入。已复现 4 次，规律如下：
 *   - PowerShell 终端直接 `pnpm build` —— 稳定，10~20 秒完成 ✅
 *   - 从 node 进程 spawn pnpm / vite —— 必卡 ❌
 *   - **即使经 pwsh 中转，只要祖先进程是 node（本脚本就是），仍会卡**（2026-09-20 实测） ❌
 * 推测与 stdio 继承/TTY 判定有关，未彻底查明。当前对策：
 *   1. 子进程一律走 pwsh，且 stdio 用 `pipe` 而非 `inherit`（减小触发面）；
 *   2. 重编前清 `node_modules/.vite`（另一已知诱因）；
 *   3. 仍卡住时 spawnSync 超时后报错退出——**此时请手动执行 `pnpm build`，
 *      再用 `pnpm build:green -- --force` 重跑本脚本**（手动那次实测 18 秒完成）。
 */

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SRC_TAURI = path.join(ROOT, 'src-tauri');
const GREEN_DIR = path.join(ROOT, 'dist', '智能待办清单-绿色版');
const EXE_SRC = path.join(SRC_TAURI, 'target', 'release', 'todolist.exe');
const EXE_DST = path.join(GREEN_DIR, '智能待办清单.exe');
const VER = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8')).version;
const FORCE = process.argv.includes('--force');
const CLEAN_DATA = process.argv.includes('--clean-data');

/** 经 PowerShell 跑命令（见文件头告警）。 */
function run(cmd, cwd, timeoutMs) {
  console.log(`\n> ${cmd}   (cwd=${cwd})`);
  const r = spawnSync(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-Command', `Set-Location '${cwd}'; ${cmd}`],
    // stdio 用 pipe 而非 inherit：inherit 会把手写管道交出去，怀疑是 vite 卡死的触发条件之一
    { encoding: 'utf8', timeout: timeoutMs }
  );
  if (r.stdout) process.stdout.write(r.stdout);
  if (r.stderr) process.stderr.write(r.stderr);
  if (r.error) {
    console.error(`\n✗ ${cmd}: ${r.error.message}`);
    process.exit(1);
  }
  if (r.status !== 0) {
    console.error(`\n✗ 失败：${cmd} (status=${r.status}${r.signal ? `, signal=${r.signal}` : ''})`);
    console.error('  若卡在 "building ssr environment"：先手动 `pnpm build`（PowerShell 里直接跑，实测 18 秒），');
    console.error('  再 `pnpm build:green -- --force` 重跑本脚本。');
    process.exit(r.status ?? 1);
  }
}

function rmrf(p) {
  if (fs.existsSync(p)) fs.rmSync(p, { recursive: true, force: true });
}

function newestMtime(dir) {
  let newest = 0;
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else newest = Math.max(newest, fs.statSync(p).mtimeMs);
    }
  };
  if (fs.existsSync(dir)) walk(dir);
  return newest;
}

// ─── 1) 前端 ───────────────────────────────────────────────────────────
// 源码没动就跳过：一是省时间，二是绕开 vite 在本机的偶发卡死。
const sourcesNewest = Math.max(
  newestMtime(path.join(ROOT, 'src')),
  newestMtime(path.join(ROOT, 'static')),
  fs.statSync(path.join(ROOT, 'svelte.config.js')).mtimeMs,
  fs.statSync(path.join(ROOT, 'vite.config.js')).mtimeMs
);
const buildIndex = path.join(ROOT, 'build', 'index.html');
const buildFresh = fs.existsSync(buildIndex) && fs.statSync(buildIndex).mtimeMs >= sourcesNewest;

if (buildFresh && !FORCE) {
  console.log('· build/ 比源码新 → 跳过前端重编（要强制重编：pnpm build:green -- --force）');
} else {
  // 依赖预构建缓存是已知的卡死诱因之一，重编前一律清掉。
  const viteCache = path.join(ROOT, 'node_modules', '.vite');
  if (fs.existsSync(viteCache)) {
    console.log('· 清理 node_modules/.vite（已知卡死诱因）');
    rmrf(viteCache);
  }
  run('pnpm build', ROOT, 240_000);
}

// ─── 2) Rust 侧：把 build/ 嵌进 exe ────────────────────────────────────
// tauri.conf.json bundle.active=false → 不会再生成 NSIS/MSI。
// ⚠️ 必须带 --features custom-protocol：否则前端资源不嵌入、tauri:// 协议不注册，
//    release 版会去连 devUrl(localhost:1420) → 打开就是 "localhost 拒绝连接"。
//    （2026-09-20 实测：Cargo.toml 里漏定义该 feature，绿色版双击必白屏。）
const RUST_FEATURES = process.argv.includes('--no-custom-protocol')
  ? ''
  : ' --features custom-protocol';
run(`cargo build --release${RUST_FEATURES}`, SRC_TAURI, 900_000);

if (!fs.existsSync(EXE_SRC)) {
  console.error(`\n✗ 未找到 ${EXE_SRC}`);
  process.exit(1);
}

// ─── 3) 组装绿色目录 ───────────────────────────────────────────────────
fs.mkdirSync(GREEN_DIR, { recursive: true });
fs.copyFileSync(EXE_SRC, EXE_DST);
if (!fs.existsSync(path.join(GREEN_DIR, '使用说明.txt'))) {
  console.error('\n✗ 缺少 使用说明.txt，请先补齐再打包');
  process.exit(1);
}
// ⚠️ data/ 是用户数据目录：默认保留，只认 `--clean-data`。
// （旧版脚本会在打包时顺手删掉它——那是把用户全部待办清空的写法，已修正。）
const dataInGreen = path.join(GREEN_DIR, 'data');
if (CLEAN_DATA && fs.existsSync(dataInGreen)) {
  console.log('· --clean-data：删除绿色目录下的 data/');
  rmrf(dataInGreen);
} else if (fs.existsSync(dataInGreen)) {
  const cnt = fs.readdirSync(dataInGreen).length;
  console.log(`· 保留绿色目录下的 data/（${cnt} 项，含用户数据，打包会一起进 zip）`);
}
console.log(`\n· exe → ${EXE_DST} (${(fs.statSync(EXE_DST).size / 1024 / 1024).toFixed(2)} MB)`);

// ─── 3.5) 前端资源嵌入自检 ─────────────────────────────────────────────
// 2026-09-20 事故：Cargo.toml 漏定义 custom-protocol → tauri-build 不嵌前端资源、
// 也不注册 tauri:// 协议，release 版回退到 devUrl，双击即 "localhost 拒绝连接"。
// 只看 exe 大小看不出来（都是 9MB 左右），这里直接查 exe 里有没有前端文件名。
function findFrontendMarker() {
  const appDir = path.join(ROOT, 'build', '_app', 'immutable');
  if (!fs.existsSync(appDir)) return null;
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) {
        const r = walk(p);
        if (r) return r;
      } else if (e.name.endsWith('.js')) return e.name;
    }
    return null;
  };
  return walk(appDir);
}
const marker = findFrontendMarker();
if (!marker) {
  console.error('\n✗ build/_app/immutable 下找不到 js 产物，前端可能没构建');
  process.exit(1);
}
if (!fs.readFileSync(EXE_DST).toString('latin1').includes(marker)) {
  console.error(`\n✗ exe 里找不到前端资源 ${marker}`);
  console.error('  说明构建时未启用 custom-protocol（前端没嵌入、tauri:// 协议没注册），');
  console.error('  这个 exe 双击会白屏并报 "localhost 拒绝连接"。');
  console.error(`  请确认 Cargo.toml 有 [features] custom-protocol 且本脚本带 --features custom-protocol。`);
  process.exit(1);
}
console.log(`· 前端资源已嵌入 exe（校验到 ${marker}）`);

// ─── 4) 红线自检 ───────────────────────────────────────────────────────
const allowed = ['智能待办清单.exe', '使用说明.txt', 'data'];
const extra = fs.readdirSync(GREEN_DIR).filter((n) => !allowed.includes(n));
if (extra.length) {
  console.error(`\n✗ 绿色目录出现多余内容：${extra.join(', ')}`);
  process.exit(1);
}
const installer = fs
  .readdirSync(path.join(ROOT, 'dist'), { recursive: true })
  .filter((n) => /\.msi$|setup\.exe$/i.test(String(n)));
if (installer.length) {
  console.error(`\n✗ dist/ 内发现安装器产物（违反红线）：${installer.join(', ')}`);
  process.exit(1);
}

// ─── 5) zip（≤10MB 红线） ──────────────────────────────────────────────
const zip = path.join(ROOT, 'dist', `智能待办清单-v${VER}-绿色版.zip`);
rmrf(zip);
run(
  `Compress-Archive -Path '${GREEN_DIR}\\*' -DestinationPath '${zip}' -CompressionLevel Optimal`,
  ROOT,
  120_000
);
const mb = fs.statSync(zip).size / 1024 / 1024;
console.log(`\n✓ 绿色版就绪：${GREEN_DIR}`);
console.log(`✓ zip：${path.basename(zip)}  ${mb.toFixed(2)} MB`);
if (mb > 10) {
  console.error('\n✗ zip 超过 10MB 红线');
  process.exit(1);
}
