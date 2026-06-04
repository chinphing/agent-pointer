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
}

const extra = process.argv.slice(2);
const result = spawnSync('npx', ['tauri', 'build', ...extra], {
  stdio: 'inherit',
  env,
  shell: process.platform === 'win32',
});

process.exit(result.status ?? 1);
