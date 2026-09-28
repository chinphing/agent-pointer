#!/usr/bin/env node
/**
 * Load pointer.local.env then run `tauri dev`.
 */
import { spawnSync } from 'node:child_process';
import { applyPointerLocalEnv } from './lib/load-pointer-local-env.mjs';

const env = { ...process.env };
applyPointerLocalEnv(env, { logPrefix: '[tauri-dev]' });

const result = spawnSync(
  'npx',
  ['tauri', 'dev', '--config', 'src-tauri/tauri.dev.conf.json', ...process.argv.slice(2)],
  {
    stdio: 'inherit',
    env,
    shell: process.platform === 'win32',
  },
);

process.exit(result.status ?? 1);
