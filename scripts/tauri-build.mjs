#!/usr/bin/env node
/**
 * Cross-platform wrapper for `tauri build`.
 * Reads TAURI_SIGNING_PRIVATE_KEY from TAURI_SIGNING_PRIVATE_KEY_PATH.
 * Linux AppImage: set NO_STRIP=true (linuxdeploy strip / .relr.dyn incompatibility).
 * Windows / macOS: unchanged env.
 */
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';

const env = { ...process.env };

// Read signing key from file path instead of inline env var
const keyPath = env.TAURI_SIGNING_PRIVATE_KEY_PATH;
if (keyPath) {
  try {
    env.TAURI_SIGNING_PRIVATE_KEY = readFileSync(keyPath, 'utf8').trim();
    console.log('[tauri-build] signing key loaded from', keyPath);
  } catch (e) {
    console.error('[tauri-build] WARNING: failed to read signing key:', e.message);
  }
}
if (process.platform === 'linux') {
  env.NO_STRIP = 'true';
  env.APPIMAGE_EXTRACT_AND_RUN = '1';
  console.log('[tauri-build] Linux: NO_STRIP=true, APPIMAGE_EXTRACT_AND_RUN=1');
}

const extra = process.argv.slice(2);
const result = spawnSync('npx', ['tauri', 'build', ...extra], {
  stdio: 'inherit',
  env,
  shell: process.platform === 'win32',
});

const status = result.status ?? 1;
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

process.exit(status);
