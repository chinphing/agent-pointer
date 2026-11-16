#!/usr/bin/env node
/** Build Vue for same-origin pointer-server (empty VITE_WEB_API_BASE). */
import { spawnSync } from 'node:child_process'

const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm'
const result = spawnSync(npm, ['run', 'build'], {
  env: {
    ...process.env,
    VITE_WEB_API_BASE: '',
    // 内网部署的目标浏览器（Chromium 63 内核，如医院统一镜像）能解析 ESM，
    // 但不支持动态 import()——入口 chunk 会在解析阶段抛
    // `Uncaught SyntaxError: Unexpected token 'import'`，整页白屏。
    // 服务端 UI 默认内联所有分块，产物里不再有 import()；桌面端构建不受影响。
    POINTER_INLINE_CHUNKS: process.env.POINTER_INLINE_CHUNKS ?? '1'
  },
  stdio: 'inherit',
  shell: process.platform === 'win32'
})

process.exit(result.status ?? 1)
