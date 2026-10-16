#!/usr/bin/env node
/**
 * Cross-platform wrapper for `tauri build`.
 * Managed client builds load or create the updater signing key.
 * Linux AppImage: set NO_STRIP=true (linuxdeploy strip / .relr.dyn incompatibility).
 * Windows / macOS: unchanged env.
 */
import { unlinkSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { applyPointerLocalEnv } from './lib/load-pointer-local-env.mjs';
import { prepareUpdaterSigning } from './lib/updater-signing-key.mjs';

const env = { ...process.env };
applyPointerLocalEnv(env, { logPrefix: '[tauri-build]' });

let prepared
try {
  prepared = await prepareUpdaterSigning({ env, argv: process.argv.slice(2) })
} catch (err) {
  console.error(`[tauri-build] ${err instanceof Error ? err.message : err}`)
  process.exit(1)
}

if (process.platform === 'linux') {
  env.NO_STRIP = 'true';
  env.APPIMAGE_EXTRACT_AND_RUN = '1';
  console.log('[tauri-build] Linux: NO_STRIP=true, APPIMAGE_EXTRACT_AND_RUN=1');
}

const extra = prepared.argv
const edition = (env.POINTER_EDITION || '').trim().toLowerCase();
const isManaged = edition === 'managed';
if (isManaged) {
  env.VITE_POINTER_EDITION = env.VITE_POINTER_EDITION || 'managed';
  console.log(`[tauri-build] POINTER_EDITION=${edition}`);
} else {
  // Standalone build: no updater artifacts, no baked-in control plane.
  extra.push('--config', 'src-tauri/tauri.personal.conf.json');
  console.log('[tauri-build] standalone build (no updater artifacts)');
}

let status = 1
try {
  const result = spawnSync('npx', ['tauri', 'build', ...extra], {
    stdio: 'inherit',
    env,
    shell: process.platform === 'win32',
  });
  status = result.status ?? 1;
  if (status !== 0 && process.platform === 'linux') {
    const bundleArgs = extra.join(' ').toLowerCase();
    const buildsAppImage = bundleArgs.length === 0 || bundleArgs.includes('appimage');
    if (buildsAppImage) {
      console.error('\n[tauri-build] AppImage 打包失败（linuxdeploy）。常见原因：');
      console.error('  • 未安装系统依赖 → bash scripts/install-linux-build-deps.sh');
      console.error('  • 直接运行 npx/cargo tauri build（未设 NO_STRIP）→ 请用 npm run tauri:build');
      console.error('  • 查看具体错误 → npm run tauri:build -- --bundles appimage --verbose\n');
    }
  }
} finally {
  if (prepared.overlayPath) {
    try {
      unlinkSync(prepared.overlayPath)
    } catch (err) {
      console.warn(
        `[tauri-build] could not remove updater pubkey overlay ${prepared.overlayPath}: ${err instanceof Error ? err.message : err}`,
      )
    }
  }
}

process.exit(status);
