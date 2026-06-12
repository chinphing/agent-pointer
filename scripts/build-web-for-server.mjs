#!/usr/bin/env node
/** Build Vue for same-origin pointer-server (empty VITE_WEB_API_BASE). */
import { spawnSync } from 'node:child_process'

const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm'
const result = spawnSync(npm, ['run', 'build'], {
  env: { ...process.env, VITE_WEB_API_BASE: '' },
  stdio: 'inherit',
  shell: process.platform === 'win32'
})

process.exit(result.status ?? 1)
