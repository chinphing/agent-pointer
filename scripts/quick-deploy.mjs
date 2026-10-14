#!/usr/bin/env node
/**
 * Bring up a local Pointer test the same way a smoke run does, for one of the
 * four deployment cells (edition × client/server).
 *
 *   node scripts/quick-deploy.mjs start standalone-server
 *   node scripts/quick-deploy.mjs start standalone-server --runtime bin
 *   node scripts/quick-deploy.mjs start managed-server --api-base URL --web-base URL --annotate-base URL
 *   node scripts/quick-deploy.mjs start standalone-client
 *   node scripts/quick-deploy.mjs start standalone-client --runtime bin
 *   node scripts/quick-deploy.mjs start managed-client --api-base URL --web-base URL --annotate-base URL
 *   node scripts/quick-deploy.mjs status
 *   node scripts/quick-deploy.mjs stop [mode]
 *   node scripts/quick-deploy.mjs plan <mode> [--runtime dev|bin]
 *
 * `--runtime dev` (the default) uses cargo / Vite / tauri dev.
 * `--runtime bin` runs the release binaries from target/release and does not
 * compile. The server binary serves repo dist/ on port 8787; the client binary
 * is pointer-app. Build them first with npm run server:build or npm run tauri:build.
 *
 * State (pids, logs, generated standalone password) lives in `.quick-deploy/`
 * and is gitignored. Dev uses `.quick-deploy/<mode>/`. Release binaries use
 * `.quick-deploy/bin/<mode>/`, so the two runtimes do not share a password or
 * pid file. The script does not read `pointer.local.env`; each mode sets its
 * own environment so a managed file cannot leak into a standalone run.
 */
import { spawn, spawnSync } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
export const REPO_ROOT = path.resolve(SCRIPT_DIR, '..')

export const MODES = [
  'standalone-server',
  'standalone-client',
  'managed-server',
  'managed-client',
]

export const RUNTIMES = ['dev', 'bin']

const SERVER_ADDR = '127.0.0.1:8787'
const WEB_PORT = 1420
const HEALTH_URL = 'http://127.0.0.1:8787/api/health'
const AUTH_MODE_URL = 'http://127.0.0.1:8787/api/auth/mode'

const CLEARED_KEYS = [
  'POINTER_EDITION',
  'VITE_POINTER_EDITION',
  'POINTER_API_BASE',
  'POINTER_WEB_BASE',
  'VITE_POINTER_WEB_BASE',
  'COMPUTER_ANNOTATE_API_BASE',
  'POINTER_DOWNLOAD_URL',
  'VITE_POINTER_DOWNLOAD_URL',
  'POINTER_DEPLOYMENT_MODE',
  'POINTER_SERVER_ADMIN_USERNAME',
  'POINTER_SERVER_ADMIN_PASSWORD_HMAC',
  'POINTER_SERVER_AUTH_HMAC_SECRET',
  'POINTER_APP_DATA_DIR',
  'POINTER_SERVER_ADDR',
  'POINTER_SERVER_STATIC_DIR',
]

export function releaseBinaryName(kind) {
  const windows = process.platform === 'win32'
  if (kind === 'server') return windows ? 'pointer-server.exe' : 'pointer-server'
  return windows ? 'pointer-app.exe' : 'pointer-app'
}

export function defaultReleaseBinary(root, kind) {
  return path.join(root, 'target', 'release', releaseBinaryName(kind))
}

export function localAdminCredentials(randomBytes = (n) => crypto.randomBytes(n)) {
  const secret = randomBytes(32).toString('hex')
  const password = randomBytes(18).toString('base64url')
  const passwordHmac = crypto.createHmac('sha256', secret).update(password).digest('hex')
  return { username: 'admin', secret, password, passwordHmac }
}

export function modeKind(mode) {
  if (!MODES.includes(mode)) {
    throw new Error(`unknown mode ${mode}; expected one of ${MODES.join(', ')}`)
  }
  const managed = mode.startsWith('managed')
  const server = mode.endsWith('server')
  return { managed, server, authMode: managed ? 'platform' : server ? 'standalone' : null }
}

function cleanEnv(base) {
  const env = { ...base }
  for (const key of CLEARED_KEYS) delete env[key]
  return env
}

function requireDomains(domains) {
  const missing = []
  if (!domains?.apiBase) missing.push('POINTER_API_BASE (--api-base)')
  if (!domains?.webBase) missing.push('POINTER_WEB_BASE (--web-base)')
  if (!domains?.annotateBase) missing.push('COMPUTER_ANNOTATE_API_BASE (--annotate-base)')
  if (missing.length) {
    throw new Error(`managed mode needs control-plane domains: ${missing.join(', ')}`)
  }
}

/**
 * Describe the processes and environment for one mode. Does not spawn anything.
 * @param {{
 *   mode: string,
 *   root?: string,
 *   stateDir: string,
 *   domains?: { apiBase?: string, webBase?: string, annotateBase?: string },
 *   runtime?: 'dev' | 'bin',
 *   credentials?: { username: string, secret: string, password: string, passwordHmac: string } | null,
 *   baseEnv?: NodeJS.ProcessEnv,
 *   binaries?: { server?: string, client?: string },
 * }} input
 */
export function buildPlan(input) {
  const mode = input.mode
  const runtime = input.runtime ?? 'dev'
  if (!RUNTIMES.includes(runtime)) {
    throw new Error(`unknown runtime ${runtime}; expected one of ${RUNTIMES.join(', ')}`)
  }
  const { managed, server, authMode } = modeKind(mode)
  const root = input.root ?? REPO_ROOT
  const stateDir = input.stateDir
  const dataDir = path.join(stateDir, 'data')
  const env = cleanEnv(input.baseEnv ?? {})
  env.POINTER_APP_DATA_DIR = dataDir

  if (managed) {
    requireDomains(input.domains)
    env.POINTER_EDITION = 'managed'
    env.VITE_POINTER_EDITION = 'managed'
    env.POINTER_API_BASE = input.domains.apiBase
    env.POINTER_WEB_BASE = input.domains.webBase
    env.VITE_POINTER_WEB_BASE = input.domains.webBase
    env.COMPUTER_ANNOTATE_API_BASE = input.domains.annotateBase
  }

  const staticDir = path.join(root, 'dist')
  const processes = []
  if (server) {
    env.POINTER_SERVER_ADDR = SERVER_ADDR
    env.POINTER_DEPLOYMENT_MODE = managed ? 'platform' : 'standalone'
    if (!managed) {
      const creds = input.credentials
      if (!creds?.username || !creds.secret || !creds.passwordHmac) {
        throw new Error('standalone-server plan requires generated admin credentials')
      }
      env.POINTER_SERVER_ADMIN_USERNAME = creds.username
      env.POINTER_SERVER_AUTH_HMAC_SECRET = creds.secret
      env.POINTER_SERVER_ADMIN_PASSWORD_HMAC = creds.passwordHmac
    }
    if (runtime === 'bin') {
      env.POINTER_SERVER_STATIC_DIR = staticDir
      processes.push({
        name: 'server',
        command: input.binaries?.server ?? defaultReleaseBinary(root, 'server'),
        args: [],
        log: path.join(stateDir, 'server.log'),
      })
    } else {
      processes.push({
        name: 'server',
        command: 'cargo',
        args: ['run', '-p', 'pointer-server'],
        log: path.join(stateDir, 'server.log'),
      })
      processes.push({
        name: 'web',
        command: 'npm',
        args: ['run', 'web:dev'],
        log: path.join(stateDir, 'web.log'),
      })
    }
  } else if (runtime === 'bin') {
    processes.push({
      name: 'client',
      command: input.binaries?.client ?? defaultReleaseBinary(root, 'client'),
      args: [],
      log: path.join(stateDir, 'client.log'),
    })
  } else {
    processes.push({
      name: 'client',
      command: 'npx',
      args: ['tauri', 'dev', '--config', 'src-tauri/tauri.dev.conf.json'],
      log: path.join(stateDir, 'client.log'),
    })
  }

  const webUrl = !server
    ? null
    : runtime === 'bin'
      ? `http://${SERVER_ADDR}/`
      : `http://127.0.0.1:${WEB_PORT}/`

  return {
    mode,
    runtime,
    root,
    stateDir,
    dataDir,
    authMode,
    staticDir,
    webUrl,
    usesVite: runtime === 'dev',
    processes: processes.map((proc) => ({ ...proc, cwd: root, env })),
  }
}

/** Why a bin plan cannot launch, or null when every artifact is present. */
export function missingLaunchArtifact(plan) {
  if (plan.runtime !== 'bin') return null
  for (const proc of plan.processes) {
    if (!fs.existsSync(proc.command)) {
      const hint = proc.name === 'server'
        ? 'Build it with npm run server:build.'
        : 'Build it with npm run tauri:build.'
      return `${proc.command} is missing. ${hint}`
    }
  }
  if (plan.authMode && !fs.existsSync(plan.staticDir)) {
    return `Web bundle ${plan.staticDir} is missing. npm run server:build writes it.`
  }
  return null
}

function usage() {
  console.error(`Usage:
  node scripts/quick-deploy.mjs start <${MODES.join('|')}> [--runtime dev|bin] [--api-base URL --web-base URL --annotate-base URL]
  node scripts/quick-deploy.mjs plan <mode> [--runtime dev|bin] [domain flags]
  node scripts/quick-deploy.mjs status
  node scripts/quick-deploy.mjs stop [mode] [--runtime dev|bin]

--runtime dev (default) compiles as it starts: cargo run, Vite, or tauri dev.
--runtime bin runs target/release binaries and does not compile.
Managed modes need the three control-plane domains (flags or POINTER_API_BASE / POINTER_WEB_BASE / COMPUTER_ANNOTATE_API_BASE).
Dev server and dev client share Vite port ${WEB_PORT}. The release server serves dist/ on ${SERVER_ADDR}.`)
}

export function parseArgs(argv) {
  const args = [...argv]
  const command = args.shift()
  if (!command || !['start', 'stop', 'status', 'plan'].includes(command)) {
    usage()
    return null
  }
  let mode = null
  if (command !== 'status') {
    const next = args[0]
    if (next && !next.startsWith('--')) {
      mode = args.shift()
    }
  }
  const domains = {}
  let stateDir = null
  let runtime = 'dev'
  while (args.length) {
    const flag = args.shift()
    const value = args.shift()
    if (!value || value.startsWith('--')) {
      throw new Error(`missing value for ${flag}`)
    }
    if (flag === '--api-base') domains.apiBase = value
    else if (flag === '--web-base') domains.webBase = value
    else if (flag === '--annotate-base') domains.annotateBase = value
    else if (flag === '--state-dir') stateDir = value
    else if (flag === '--runtime') runtime = value
    else throw new Error(`unknown flag ${flag}`)
  }
  if (!RUNTIMES.includes(runtime)) {
    throw new Error(`unknown runtime ${runtime}; expected one of ${RUNTIMES.join(', ')}`)
  }
  domains.apiBase ||= process.env.POINTER_API_BASE
  domains.webBase ||= process.env.POINTER_WEB_BASE
  domains.annotateBase ||= process.env.COMPUTER_ANNOTATE_API_BASE
  return { command, mode, domains, stateDir, runtime }
}

function defaultStateRoot() {
  return path.join(REPO_ROOT, '.quick-deploy')
}

function modeStateDir(mode, runtime, override) {
  if (override) return path.resolve(override)
  // Dev stays at .quick-deploy/<mode> so an already-running dev stack keeps its
  // pid file and generated password. Release binaries get their own tree.
  if (runtime === 'bin') return path.join(defaultStateRoot(), 'bin', mode)
  return path.join(defaultStateRoot(), mode)
}

function recordedRuns(filter = {}) {
  const runs = []
  for (const runtime of RUNTIMES) {
    if (filter.runtime && filter.runtime !== runtime) continue
    for (const mode of MODES) {
      if (filter.mode && filter.mode !== mode) continue
      const stateDir = modeStateDir(mode, runtime)
      if (fs.existsSync(pidFile(stateDir)) || fs.existsSync(stateDir)) {
        runs.push({ mode, runtime, stateDir })
      }
    }
  }
  return runs
}

function pidFile(stateDir) {
  return path.join(stateDir, 'pids.json')
}

function readPids(stateDir) {
  const file = pidFile(stateDir)
  if (!fs.existsSync(file)) return {}
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'))
  } catch {
    return {}
  }
}

function writePids(stateDir, pids) {
  fs.writeFileSync(pidFile(stateDir), JSON.stringify(pids, null, 2))
}

function isAlive(pid) {
  if (!pid) return false
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

function portOpen(port, host = '127.0.0.1') {
  return new Promise((resolve) => {
    const socket = net.connect({ port, host }, () => {
      socket.end()
      resolve(true)
    })
    socket.on('error', () => resolve(false))
  })
}

function credentialsFile(stateDir) {
  return path.join(stateDir, 'standalone-admin.env')
}

function readCredentials(stateDir) {
  const file = credentialsFile(stateDir)
  if (!fs.existsSync(file)) return null
  const out = {}
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    const trimmed = line.trim()
    if (!trimmed || trimmed.startsWith('#')) continue
    const eq = trimmed.indexOf('=')
    if (eq === -1) continue
    out[trimmed.slice(0, eq)] = trimmed.slice(eq + 1)
  }
  if (!out.POINTER_SERVER_ADMIN_USERNAME || !out.POINTER_SERVER_AUTH_HMAC_SECRET || !out.POINTER_SERVER_ADMIN_PASSWORD_HMAC) {
    return null
  }
  return {
    username: out.POINTER_SERVER_ADMIN_USERNAME,
    secret: out.POINTER_SERVER_AUTH_HMAC_SECRET,
    password: out.POINTER_SERVER_ADMIN_PASSWORD || '',
    passwordHmac: out.POINTER_SERVER_ADMIN_PASSWORD_HMAC,
    reused: true,
  }
}

function writeCredentials(stateDir, creds) {
  const body = [
    `POINTER_SERVER_ADMIN_USERNAME=${creds.username}`,
    `POINTER_SERVER_ADMIN_PASSWORD=${creds.password}`,
    `POINTER_SERVER_AUTH_HMAC_SECRET=${creds.secret}`,
    `POINTER_SERVER_ADMIN_PASSWORD_HMAC=${creds.passwordHmac}`,
    '',
  ].join('\n')
  fs.writeFileSync(credentialsFile(stateDir), body, { mode: 0o600 })
}

function killPid(pid) {
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore' })
    return
  }
  try {
    process.kill(-pid, 'SIGTERM')
  } catch {
    try {
      process.kill(pid, 'SIGTERM')
    } catch {
      /* already gone */
    }
  }
}

function stopMode(mode, stateDir) {
  const pids = readPids(stateDir)
  const names = Object.keys(pids)
  if (!names.length) {
    console.log(`[quick-deploy] ${mode}: not running`)
    return
  }
  for (const name of names) {
    const pid = pids[name]
    if (isAlive(pid)) {
      killPid(pid)
      console.log(`[quick-deploy] ${mode}: stopped ${name} (pid ${pid})`)
    } else {
      console.log(`[quick-deploy] ${mode}: ${name} pid ${pid} already exited`)
    }
  }
  writePids(stateDir, {})
}

async function waitFor(label, probe, timeoutMs) {
  const deadline = Date.now() + timeoutMs
  let last = ''
  while (Date.now() < deadline) {
    try {
      const ok = await probe()
      if (ok) return
    } catch (err) {
      last = err instanceof Error ? err.message : String(err)
    }
    await new Promise((resolve) => setTimeout(resolve, 1000))
  }
  throw new Error(`timed out waiting for ${label}${last ? `: ${last}` : ''}`)
}

async function assertAuthMode(expected) {
  const response = await fetch(AUTH_MODE_URL)
  if (!response.ok) throw new Error(`auth mode HTTP ${response.status}`)
  const body = await response.json()
  if (body.mode !== expected) {
    throw new Error(`auth mode is ${body.mode}, expected ${expected}`)
  }
  return true
}

function spawnLogged(proc) {
  fs.mkdirSync(path.dirname(proc.log), { recursive: true })
  const logFd = fs.openSync(proc.log, 'a')
  const child = spawn(proc.command, proc.args, {
    cwd: proc.cwd,
    env: proc.env,
    detached: process.platform !== 'win32',
    stdio: ['ignore', logFd, logFd],
    shell: process.platform === 'win32',
  })
  child.unref()
  fs.closeSync(logFd)
  return child.pid
}

function tailFile(file, lines = 40) {
  if (!fs.existsSync(file)) return ''
  const text = fs.readFileSync(file, 'utf8')
  return text.split('\n').slice(-lines).join('\n')
}

async function startMode(mode, runtime, domains, stateDirOverride) {
  const stateDir = modeStateDir(mode, runtime, stateDirOverride)
  fs.mkdirSync(stateDir, { recursive: true })
  const existing = readPids(stateDir)
  if (Object.values(existing).some((pid) => isAlive(pid))) {
    console.log(`[quick-deploy] ${runtime} ${mode} is already running. Use status or stop first.`)
    return
  }

  const { server } = modeKind(mode)
  if (server && await portOpen(8787)) {
    throw new Error('127.0.0.1:8787 is already in use. Stop the other pointer-server first.')
  }
  if (runtime === 'dev' && await portOpen(WEB_PORT)) {
    throw new Error(`127.0.0.1:${WEB_PORT} is already in use. Dev server web and tauri dev share this Vite port.`)
  }

  let credentials = null
  let createdPassword = false
  if (mode === 'standalone-server') {
    credentials = readCredentials(stateDir)
    if (!credentials) {
      credentials = localAdminCredentials()
      createdPassword = true
    }
  }

  const plan = buildPlan({
    mode,
    runtime,
    stateDir,
    domains,
    credentials,
    baseEnv: process.env,
  })
  const missing = missingLaunchArtifact(plan)
  if (missing) throw new Error(missing)
  if (createdPassword) writeCredentials(stateDir, credentials)
  fs.mkdirSync(plan.dataDir, { recursive: true })

  const pids = {}
  for (const proc of plan.processes) {
    const pid = spawnLogged(proc)
    pids[proc.name] = pid
    console.log(`[quick-deploy] ${runtime} ${mode}: started ${proc.name} (pid ${pid}), log ${proc.log}`)
  }
  writePids(stateDir, pids)

  try {
    if (server) {
      const healthTimeout = runtime === 'bin' ? 30_000 : 900_000
      console.log(runtime === 'bin'
        ? '[quick-deploy] waiting for the release server on /api/health'
        : '[quick-deploy] waiting for /api/health (a first cargo build can take several minutes)')
      await waitFor('server health', async () => {
        const response = await fetch(HEALTH_URL)
        return response.ok && (await response.text()).trim() === 'ok'
      }, healthTimeout)
      await waitFor('auth mode', () => assertAuthMode(plan.authMode), 30_000)
      await waitFor('web ui', async () => {
        const response = await fetch(plan.webUrl)
        return response.ok
      }, runtime === 'bin' ? 15_000 : 60_000)
      console.log(`[quick-deploy] ${runtime} ${mode} is up`)
      console.log(`[quick-deploy] web ${plan.webUrl}`)
      console.log(`[quick-deploy] auth mode ${plan.authMode}`)
      console.log(`[quick-deploy] data ${plan.dataDir}`)
      if (mode === 'standalone-server') {
        const file = credentialsFile(stateDir)
        console.log(`[quick-deploy] username ${credentials.username}`)
        console.log(`[quick-deploy] credentials ${file}`)
        if (createdPassword) {
          console.log(`[quick-deploy] password ${credentials.password}`)
        } else {
          console.log('[quick-deploy] reused the existing standalone password in that file')
        }
      } else {
        console.log('[quick-deploy] platform mode uses control-plane login; the domains were passed through as environment variables')
      }
    } else {
      if (runtime === 'bin') {
        const clientPid = pids.client
        await new Promise((resolve) => setTimeout(resolve, 1500))
        if (!isAlive(clientPid)) throw new Error('pointer-app exited during startup')
        console.log(`[quick-deploy] ${runtime} ${mode} is running (pid ${clientPid})`)
      } else {
        console.log('[quick-deploy] waiting for the desktop dev server on port 1420')
        await waitFor('desktop vite', () => portOpen(WEB_PORT), 900_000)
        console.log(`[quick-deploy] ${runtime} ${mode} dev server is listening on http://127.0.0.1:${WEB_PORT}/`)
      }
      console.log(`[quick-deploy] data ${plan.dataDir}`)
    }
  } catch (err) {
    console.error(`[quick-deploy] ${runtime} ${mode} failed to become ready: ${err instanceof Error ? err.message : err}`)
    for (const proc of plan.processes) {
      console.error(`--- ${proc.log} ---`)
      console.error(tailFile(proc.log))
    }
    stopMode(mode, stateDir)
    throw err
  }
}

async function status() {
  const runs = recordedRuns()
  if (!runs.length) {
    console.log('[quick-deploy] nothing recorded under .quick-deploy/')
    return
  }
  for (const { mode, runtime, stateDir } of runs) {
    const pids = readPids(stateDir)
    const alive = Object.entries(pids).filter(([, pid]) => isAlive(pid))
    if (!alive.length) {
      console.log(`[quick-deploy] ${runtime} ${mode}: stopped`)
      continue
    }
    console.log(`[quick-deploy] ${runtime} ${mode}: ${alive.map(([name, pid]) => `${name}=${pid}`).join(' ')}`)
    if (mode.endsWith('server') && await portOpen(8787)) {
      try {
        const response = await fetch(AUTH_MODE_URL)
        const body = await response.json()
        console.log(`[quick-deploy] ${runtime} ${mode}: /api/auth/mode ${body.mode}`)
      } catch (err) {
        console.log(`[quick-deploy] ${runtime} ${mode}: auth mode unavailable (${err instanceof Error ? err.message : err})`)
      }
    }
  }
}

async function main() {
  const parsed = parseArgs(process.argv.slice(2))
  if (!parsed) process.exit(1)
  const { command, mode, domains, stateDir, runtime } = parsed
  if (command === 'status') {
    await status()
    return
  }
  if (command === 'stop') {
    if (stateDir) {
      if (!mode) throw new Error('stop --state-dir needs a mode')
      modeKind(mode)
      stopMode(`${runtime} ${mode}`, modeStateDir(mode, runtime, stateDir))
      return
    }
    if (mode) modeKind(mode)
    const limitRuntime = process.argv.includes('--runtime')
    const selected = recordedRuns({
      mode: mode ?? undefined,
      runtime: limitRuntime ? runtime : undefined,
    })
    if (!selected.length) {
      console.log('[quick-deploy] nothing to stop')
      return
    }
    for (const run of selected) stopMode(`${run.runtime} ${run.mode}`, run.stateDir)
    return
  }
  if (!mode) {
    usage()
    process.exit(1)
  }
  modeKind(mode)
  if (command === 'plan') {
    const credentials = mode === 'standalone-server'
      ? localAdminCredentials(() => Buffer.alloc(32, 1))
      : null
    const plan = buildPlan({
      mode,
      runtime,
      stateDir: modeStateDir(mode, runtime, stateDir),
      domains,
      credentials,
      baseEnv: process.env,
    })
    const printable = {
      mode: plan.mode,
      runtime: plan.runtime,
      authMode: plan.authMode,
      webUrl: plan.webUrl,
      dataDir: plan.dataDir,
      processes: plan.processes.map((proc) => ({
        name: proc.name,
        command: [proc.command, ...proc.args].join(' '),
        log: proc.log,
        edition: proc.env.POINTER_EDITION ?? '',
        deploymentMode: proc.env.POINTER_DEPLOYMENT_MODE ?? '',
        apiBase: proc.env.POINTER_API_BASE ?? '',
        staticDir: proc.env.POINTER_SERVER_STATIC_DIR ?? '',
      })),
    }
    console.log(JSON.stringify(printable, null, 2))
    return
  }
  await startMode(mode, runtime, domains, stateDir)
}

const invokedDirectly = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (invokedDirectly) {
  main().catch((err) => {
    console.error(`[quick-deploy] ${err instanceof Error ? err.message : err}`)
    process.exit(1)
  })
}
