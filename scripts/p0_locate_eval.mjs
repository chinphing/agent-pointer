#!/usr/bin/env node
/**
 * P0 — `locate_element` 真实界面验证工具。
 *
 * 用法：
 *   node scripts/p0_locate_eval.mjs                 # 逐个跑 20 个场景（交互）
 *   node scripts/p0_locate_eval.mjs --only 1,5,9    # 只跑指定场景
 *   node scripts/p0_locate_eval.mjs --score         # 读真值算命中率
 *   node scripts/p0_locate_eval.mjs --reset         # 清空本轮结果
 *
 * 环境变量：
 *   POINTER_BASE   服务端地址，默认 http://127.0.0.1:8787
 *
 * 产物（全部在 cases/p0/ 下）：
 *   results.json   每次调用的原始返回 + 耗时
 *   shots/NN.jpg   调用前的整屏截图（用于事后标注真值）
 *   truth.csv      真值 bbox 模板（id,x1,y1,x2,y2），需要人工填
 *
 * 判定口径：
 *   命中      = ok=true 且 (x,y) 落在 truth.csv 的 bbox 内
 *   假确认    = ok=true 但坐标不在 bbox 内（比 not_found 更严重）
 *   失败      = ok=false
 *   门槛      = 命中率 ≥ 70%（对标 ScreenSpot-Pro ab100 的 B5 79.0%）
 */
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline/promises';
import { execFileSync } from 'node:child_process';
import { stdin as input, stdout as output } from 'node:process';

const BASE = (process.env.POINTER_BASE || 'http://127.0.0.1:8787').replace(/\/$/, '');
const ROOT = process.cwd();
const OUT = path.join(ROOT, 'cases', 'p0');
const SHOTS = path.join(OUT, 'shots');
const CROPS = path.join(OUT, 'crops');
const RESULTS = path.join(OUT, 'results.json');
const TRUTH = path.join(OUT, 'truth.csv');
/** `xcap:N` — pin every capture to one display instead of following the cursor. */
const MONITOR = process.env.POINTER_MONITOR || '';

const SCENES = [
  // 左上角菜单栏
  { id: 1, scene: '菜单栏', description: 'Apple 菜单图标', difficulty: '易' },
  { id: 2, scene: '菜单栏', description: '菜单栏里的应用名「访达」', difficulty: '易' },
  { id: 3, scene: '菜单栏', description: '菜单栏里的「文件」菜单项', difficulty: '易' },
  { id: 4, scene: '菜单栏', description: '菜单栏里的「编辑」菜单项', difficulty: '中' },
  { id: 5, scene: '菜单栏', description: '菜单栏里的「显示」菜单项', difficulty: '中' },
  // 右上角密集状态图标
  { id: 6, scene: '状态栏', description: '菜单栏右上角的 Wi-Fi 图标', difficulty: '中' },
  { id: 7, scene: '状态栏', description: '菜单栏右上角的控制中心图标', difficulty: '中' },
  { id: 8, scene: '状态栏', description: '菜单栏右上角的电池图标', difficulty: '中' },
  { id: 9, scene: '状态栏', description: '菜单栏右上角的搜索图标', difficulty: '难' },
  { id: 10, scene: '状态栏', description: '菜单栏最右侧的日期时间', difficulty: '中' },
  // 底部 Dock 大图标
  { id: 11, scene: 'Dock', description: '底部 Dock 最左边的访达图标', difficulty: '中' },
  { id: 12, scene: 'Dock', description: '底部 Dock 里的启动台图标', difficulty: '中' },
  { id: 13, scene: 'Dock', description: '底部 Dock 里的 Google Chrome 图标', difficulty: '中' },
  { id: 14, scene: 'Dock', description: '底部 Dock 里的备忘录图标', difficulty: '难' },
  { id: 15, scene: 'Dock', description: '底部 Dock 里的 App Store 图标', difficulty: '难' },
  // 桌面图标
  { id: 16, scene: '桌面', description: '桌面上的 Pointer素材 文件夹图标', difficulty: '易' },
  { id: 17, scene: '桌面', description: '桌面上的 智能填报.svg 文件图标', difficulty: '难' },
  { id: 18, scene: '桌面', description: '桌面上的 鹅娜.svg 文件图标', difficulty: '难' },
  { id: 19, scene: '桌面', description: '桌面上的 新疆差旅 文件夹图标', difficulty: '中' },
  { id: 20, scene: '桌面', description: '桌面上的 参考代码 文件夹图标', difficulty: '中' },
];

function ensureDirs() {
  fs.mkdirSync(SHOTS, { recursive: true });
}

async function api(pathname, init) {
  let res;
  try {
    res = await fetch(BASE + pathname, init);
  } catch (e) {
    throw new Error(`连不上 ${BASE}${pathname}：${e.message}（服务端起了吗？）`);
  }
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(`${pathname} -> HTTP ${res.status} ${text.slice(0, 300)}`);
  }
  return res;
}

async function locate(description, saveCapturePath) {
  const body = { elements: [{ description }] };
  if (MONITOR) body.monitorId = MONITOR;
  if (saveCapturePath) body.saveCapture = saveCapturePath;
  const res = await api('/api/computer/locate', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  return res.json();
}

/**
 * Crop around a returned box with `screencapture -R`.
 *
 * This is the check that does not depend on estimated ground truth: if the
 * target shows up in the crop, the coordinates were right.
 */
function cropAround(id, box, scale = 1.5) {
  if (!Array.isArray(box) || box.length !== 4) return null;
  const [x1, y1, x2, y2] = box;
  const cx = (x1 + x2) / 2;
  const cy = (y1 + y2) / 2;
  const w = Math.max((x2 - x1) * scale, 80);
  const h = Math.max((y2 - y1) * scale, 80);
  const left = Math.max(0, Math.round(cx - w / 2));
  const top = Math.max(0, Math.round(cy - h / 2));
  fs.mkdirSync(CROPS, { recursive: true });
  const out = path.join(CROPS, `${String(id).padStart(2, '0')}.png`);
  try {
    execFileSync('screencapture', ['-x', '-R', `${left},${top},${Math.round(w)},${Math.round(h)}`, out]);
    return out;
  } catch (e) {
    console.error('  裁剪失败:', e.message);
    return null;
  }
}

function loadResults() {
  if (!fs.existsSync(RESULTS)) {
    return { base: BASE, startedAt: new Date().toISOString(), runs: [] };
  }
  try {
    return JSON.parse(fs.readFileSync(RESULTS, 'utf8'));
  } catch {
    return { base: BASE, startedAt: new Date().toISOString(), runs: [] };
  }
}

function saveResults(r) {
  ensureDirs();
  fs.writeFileSync(RESULTS, JSON.stringify(r, null, 2) + '\n');
}

function writeTruthTemplate() {
  if (fs.existsSync(TRUTH)) return;
  const lines = ['id,x1,y1,x2,y2'];
  for (const s of SCENES) lines.push(`${s.id},,,,`);
  fs.writeFileSync(TRUTH, lines.join('\n') + '\n');
}

async function runInteractive(only) {
  ensureDirs();
  writeTruthTemplate();
  const results = loadResults();
  const rl = readline.createInterface({ input, output });
  const targets = only.length ? SCENES.filter((s) => only.includes(s.id)) : SCENES;

  console.log(`服务端: ${BASE}`);
  console.log(`本轮 ${targets.length} 个场景；截图存 ${path.relative(ROOT, SHOTS)}/`);

  for (const s of targets) {
    console.log(`\n[${s.id}] ${s.scene} — ${s.description}  (${s.difficulty})`);
    const answer = AUTO
      ? ''
      : await rl.question('  把目标显示在屏幕上，回车开始（输入 q 跳过）> ');
    if (answer.trim().toLowerCase() === 'q') {
      console.log('  已跳过');
      continue;
    }

    const t0 = Date.now();

    let payload = null;
    let error = null;
    const capturePath = path.join(SHOTS, `${String(s.id).padStart(2, '0')}_locate_view.jpg`);
    try {
      payload = await locate(s.description, capturePath);
    } catch (e) {
      error = e.message;
    }
    const elapsedMs = Date.now() - t0;
    const item = payload?.results?.[0] ?? null;
    const cropPath = item?.ok && item.box ? cropAround(s.id, item.box) : null;

    results.runs = results.runs.filter((x) => x.id !== s.id);
    results.runs.push({
      id: s.id,
      scene: s.scene,
      description: s.description,
      difficulty: s.difficulty,
      at: new Date().toISOString(),
      elapsedMs,
      ok: item?.ok ?? false,
      x: item?.x ?? null,
      y: item?.y ?? null,
      box: item?.box ?? null,
      confidence: item?.confidence ?? null,
      rounds: item?.rounds ?? null,
      reason: item?.reason ?? null,
      whatISaw: item?.what_i_saw ?? null,
      crop: cropPath ? path.relative(ROOT, cropPath) : null,
      error,
    });
    saveResults(results);

    if (error) console.log(`  ✗ 调用失败: ${error}`);
    else if (item?.ok) console.log(`  ✓ (${item.x}, ${item.y})  rounds=${item.rounds}  ${elapsedMs}ms`);
    else console.log(`  ✗ ${item?.reason ?? 'unknown'}  ${elapsedMs}ms`);
  }

  rl.close();
  console.log(`\n结果: ${path.relative(ROOT, RESULTS)}`);
  console.log(`截图: ${path.relative(ROOT, SHOTS)}/`);
  console.log(`下一步: 在 ${path.relative(ROOT, TRUTH)} 里填每行目标的真值 bbox，然后跑`);
  console.log('        node scripts/p0_locate_eval.mjs --score');
}

function readTruth() {
  const map = new Map();
  if (!fs.existsSync(TRUTH)) return map;
  const lines = fs.readFileSync(TRUTH, 'utf8').split('\n');
  for (const line of lines.slice(1)) {
    const cells = line.split(',').map((c) => c.trim());
    if (!cells[0]) continue;
    const id = Number(cells[0]);
    const nums = cells.slice(1, 5).map(Number);
    if (nums.length === 4 && nums.every(Number.isFinite)) map.set(id, nums);
  }
  return map;
}

function score() {
  if (!fs.existsSync(RESULTS)) {
    console.error(`没有 ${path.relative(ROOT, RESULTS)}，先跑一轮采集。`);
    process.exit(1);
  }
  const results = loadResults();
  const truth = readTruth();
  if (truth.size === 0) {
    console.error(`${path.relative(ROOT, TRUTH)} 里还没有任何真值 bbox。`);
    process.exit(1);
  }

  const rows = [...results.runs]
    .sort((a, b) => a.id - b.id)
    .map((run) => {
      const t = truth.get(run.id) ?? null;
      let hit = null;
      let falseConfirm = null;
      if (t && run.ok && run.x != null && run.y != null) {
        hit = run.x >= t[0] && run.x <= t[2] && run.y >= t[1] && run.y <= t[3];
        falseConfirm = !hit;
      }
      return { ...run, truth: t, hit, falseConfirm };
    });

  const scored = rows.filter((r) => r.truth);
  const n = scored.length;
  const hits = scored.filter((r) => r.hit === true).length;
  const falseConfirms = scored.filter((r) => r.falseConfirm === true).length;
  const failures = scored.filter((r) => r.ok === false).length;
  const withRounds = scored.filter((r) => r.rounds != null);
  const avgRounds = withRounds.length
    ? withRounds.reduce((s, r) => s + r.rounds, 0) / withRounds.length
    : 0;
  const avgMs = n ? scored.reduce((s, r) => s + (r.elapsedMs ?? 0), 0) / n : 0;

  const pad = (s, w) => String(s ?? '').padEnd(w);
  console.log('\n#  场景              描述                          ok    x,y            命中  假确认 rounds');
  console.log('─'.repeat(104));
  for (const r of rows) {
    const flag = r.hit === true ? '✓' : r.hit === false ? '✗' : '—';
    const fc = r.falseConfirm === true ? '⚠' : '';
    console.log(
      `${pad(r.id, 3)}${pad(r.scene, 17)}${pad(r.description, 28)}${pad(r.ok ? 'true' : 'false', 6)}` +
        `${pad(r.ok ? `${r.x},${r.y}` : (r.reason ?? '—'), 15)}${pad(flag, 6)}${pad(fc, 6)}${r.rounds ?? '—'}`
    );
  }

  console.log('─'.repeat(104));
  console.log(`已评分        ${n}/${rows.length}`);
  console.log(`命中          ${hits}/${n}${n ? ` = ${((hits / n) * 100).toFixed(1)}%` : ''}   门槛 ≥70%`);
  console.log(`假确认        ${falseConfirms}/${n}${n ? ` = ${((falseConfirms / n) * 100).toFixed(1)}%` : ''}`);
  console.log(`失败          ${failures}/${n}`);
  console.log(`平均 rounds   ${avgRounds.toFixed(2)}`);
  console.log(`平均耗时      ${(avgMs / 1000).toFixed(1)}s`);

  const unscored = rows.filter((r) => !r.truth).map((r) => r.id);
  if (unscored.length) {
    console.log(`\n还没填真值的场景: ${unscored.join(', ')}`);
  }
  if (n > 0) {
    const pass = hits / n >= 0.7;
    console.log(`\n结论: ${pass ? '通过' : '未通过'}（命中率 ${((hits / n) * 100).toFixed(1)}% vs 门槛 70%）`);
    if (n < 20) {
      console.log('注意: 样本不足 20 条。设计文档明确记录「<20 条样本正收益 → 全量归零或倒退」，结论只能做初筛。');
    }
  }
}

function reset() {
  for (const p of [RESULTS, TRUTH]) {
    if (fs.existsSync(p)) fs.unlinkSync(p);
  }
  console.log('已清空 results.json / truth.csv（截图保留）');
}

const args = process.argv.slice(2);
/** `--auto`: skip the per-scene Enter prompt (unattended run). */
const AUTO = args.includes('--auto');
const onlyIdx = args.indexOf('--only');
const only = onlyIdx >= 0 && args[onlyIdx + 1]
  ? args[onlyIdx + 1].split(',').map((s) => Number(s.trim())).filter(Number.isFinite)
  : [];

if (args.includes('--reset')) {
  reset();
} else if (args.includes('--score')) {
  score();
} else if (args.includes('--help') || args.includes('-h')) {
  console.log(fs.readFileSync(new URL(import.meta.url), 'utf8').split('*/')[0]);
} else {
  runInteractive(only).catch((e) => {
    console.error(e.message || e);
    process.exit(1);
  });
}
