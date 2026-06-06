#!/usr/bin/env node
/**
 * Cross-platform wrapper for `tauri build`.
 * Linux AppImage: set NO_STRIP=true (linuxdeploy strip / .relr.dyn incompatibility).
 * Windows / macOS: unchanged env.
 */
import { spawnSync } from 'node:child_process';

const env = { ...process.env };
if (process.platform === 'linux') {
  env.NO_STRIP = 'true';
  env.APPIMAGE_EXTRACT_AND_RUN = '1';
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
