#!/usr/bin/env node
/**
 * Cross-platform pointer-server process manager (start / stop / restart / status).
 * Runs the release binary from target/release with a PID file beside the binary.
 */
import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { syncServerDeployScripts } from './lib/sync-server-deploy-scripts.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '..')
const BIN_NAME = process.platform === 'win32' ? 'pointer-server.exe' : 'pointer-server'
const BIN_DIR = path.join(ROOT, 'target', 'release')
const BIN_PATH = path.join(BIN_DIR, BIN_NAME)
const PID_FILE = path.join(BIN_DIR, '.pointer-server.pid')
const LOG_FILE = path.join(BIN_DIR, 'pointer-server.log')

const command = process.argv[2] ?? 'status'

function usage() {
  console.error(`Usage: node scripts/pointer-server-process.mjs <start|stop|restart|status>`)
  process.exit(1)
}

function ensureBinary() {
  if (!fs.existsSync(BIN_PATH)) {
    console.error(`[pointer-server] Binary not found: ${BIN_PATH}`)
    console.error('[pointer-server] Run npm run server:build first.')
    process.exit(1)
  }
}

function readPid() {
  if (!fs.existsSync(PID_FILE)) return null
  const raw = fs.readFileSync(PID_FILE, 'utf8').trim()
  const pid = Number.parseInt(raw, 10)
  return Number.isFinite(pid) && pid > 0 ? pid : null
}

function isAlive(pid) {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

function removePidFile() {
  if (fs.existsSync(PID_FILE)) fs.unlinkSync(PID_FILE)
}

function stopProcess() {
  const pid = readPid()
  if (!pid) {
    console.log('[pointer-server] Not running (no PID file).')
    return 0
  }
  if (!isAlive(pid)) {
    console.log(`[pointer-server] Stale PID file (${pid}); removing.`)
    removePidFile()
    return 0
  }

  if (process.platform === 'win32') {
    const result = spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'inherit' })
    if (result.status !== 0) {
      console.error(`[pointer-server] Failed to stop PID ${pid}.`)
      return result.status ?? 1
    }
  } else {
    process.kill(pid, 'SIGTERM')
    const deadline = Date.now() + 10_000
    while (Date.now() < deadline) {
      if (!isAlive(pid)) break
      spawnSync('sleep', ['0.2'])
    }
    if (isAlive(pid)) {
      process.kill(pid, 'SIGKILL')
    }
  }

  removePidFile()
  console.log(`[pointer-server] Stopped (PID ${pid}).`)
  return 0
}

function startProcess() {
  ensureBinary()
  syncServerDeployScripts(BIN_DIR)

  const existing = readPid()
  if (existing && isAlive(existing)) {
    console.log(`[pointer-server] Already running (PID ${existing}).`)
    return 0
  }
  if (existing) removePidFile()

  fs.mkdirSync(BIN_DIR, { recursive: true })
  const logFd = fs.openSync(LOG_FILE, 'a')

  const child = spawn(BIN_PATH, [], {
    cwd: BIN_DIR,
    detached: true,
    stdio: ['ignore', logFd, logFd],
    windowsHide: true,
    env: { ...process.env },
  })
  fs.closeSync(logFd)

  child.unref()
  fs.writeFileSync(PID_FILE, String(child.pid), 'utf8')
  console.log(`[pointer-server] Started (PID ${child.pid}).`)
  console.log(`[pointer-server] Log: ${LOG_FILE}`)
  console.log(`[pointer-server] Working directory: ${BIN_DIR}`)
  return 0
}

function statusProcess() {
  const pid = readPid()
  if (!pid) {
    console.log('[pointer-server] Status: stopped')
    return 1
  }
  if (!isAlive(pid)) {
    console.log(`[pointer-server] Status: stopped (stale PID ${pid})`)
    removePidFile()
    return 1
  }
  console.log(`[pointer-server] Status: running (PID ${pid})`)
  console.log(`[pointer-server] Log: ${LOG_FILE}`)
  return 0
}

switch (command) {
  case 'start':
    process.exit(startProcess())
    break
  case 'stop':
    process.exit(stopProcess())
    break
  case 'restart': {
    stopProcess()
    process.exit(startProcess())
    break
  }
  case 'status':
    process.exit(statusProcess())
    break
  default:
    usage()
}
