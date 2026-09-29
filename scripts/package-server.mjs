#!/usr/bin/env node
/**
 * Package pointer-server release binary, dist/, and config example into a zip.
 * Invoked automatically at the end of `npm run server:build` (zip, all platforms).
 * Linux .deb is separate: `npm run server:deb` or `npm run server:build:linux`.
 */
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { platformTag } from './lib/platform-tag.mjs'
import { syncServerDeployScripts } from './lib/sync-server-deploy-scripts.mjs'
import { assertNoForbiddenReleasePaths } from './lib/release-hygiene.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '..')
const BIN_NAME = process.platform === 'win32' ? 'pointer-server.exe' : 'pointer-server'
const BIN_PATH = path.join(ROOT, 'target', 'release', BIN_NAME)
const DIST_DIR = path.join(ROOT, 'dist')
const SKILLS_DIR = path.join(ROOT, 'skills')
const CONFIG_EXAMPLE = path.join(ROOT, 'server', 'pointer-server.toml.example')

function ensureInputs() {
  const missing = []
  if (!fs.existsSync(BIN_PATH)) missing.push(BIN_PATH)
  if (!fs.existsSync(DIST_DIR)) missing.push(DIST_DIR)
  if (!fs.existsSync(SKILLS_DIR)) missing.push(SKILLS_DIR)
  if (!fs.existsSync(CONFIG_EXAMPLE)) missing.push(CONFIG_EXAMPLE)
  if (missing.length > 0) {
    console.error('[package-server] Missing required inputs:')
    for (const item of missing) console.error(`  • ${item}`)
    process.exit(1)
  }
}

function copyTree(src, dest) {
  fs.cpSync(src, dest, { recursive: true })
}

/** Antivirus scanners hold a freshly copied 55 MB binary for a few seconds — retry past that. */
const WIN32_ZIP_ATTEMPTS = 4
const WIN32_ZIP_RETRY_DELAY_MS = 3000

/** Synchronous sleep (no deps): lets a virus scanner release the freshly copied binary. */
function sleepSync(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms)
}

/** Entry names inside a zip, or null when the archive cannot be read. */
function listZipEntries(zipPath) {
  const result = spawnSync('tar', ['-tf', zipPath], {
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  })
  if (result.status !== 0) return null
  return (result.stdout ?? '')
    .split(/\r?\n/)
    .filter(line => line.length > 0)
}

/**
 * Zip the staging tree with the bundled bsdtar (`C:\Windows\System32\tar.exe`).
 *
 * `Compress-Archive` only reports non-terminating errors for a single locked file, so
 * powershell.exe still exits 0 and the bundle silently ships without the 55 MB binary
 * (antivirus scanning it right after the copy). bsdtar exits non-zero on a locked input,
 * and the finished archive is verified afterwards — a partial zip is deleted, never kept.
 */
function createZipWindows(stagingRoot, zipPath, innerDirName) {
  const tarCheck = spawnSync('tar', ['--version'], { stdio: 'ignore' })
  if (tarCheck.status !== 0) {
    console.error(
      '[package-server] `tar` not found. Windows 10 1803+ bundles C:\\Windows\\System32\\tar.exe.'
    )
    process.exit(1)
  }

  const requiredEntry = `${innerDirName}/${BIN_NAME}`
  for (let attempt = 1; attempt <= WIN32_ZIP_ATTEMPTS; attempt += 1) {
    const result = spawnSync('tar', ['-a', '-c', '-f', zipPath, '-C', stagingRoot, innerDirName], {
      stdio: 'inherit',
    })
    const entries = result.status === 0 ? listZipEntries(zipPath) : null
    if (entries !== null && entries.includes(requiredEntry)) return

    const reason =
      result.status !== 0
        ? `tar exited with code ${result.status ?? 'null'}`
        : `archive is missing ${requiredEntry}`
    fs.rmSync(zipPath, { force: true })
    if (attempt === WIN32_ZIP_ATTEMPTS) {
      console.error(
        `[package-server] Zip creation failed (${reason}) after ${WIN32_ZIP_ATTEMPTS} attempts.`
      )
      console.error('[package-server] No zip was kept — check the locked file and re-run the build.')
      process.exit(1)
    }
    console.warn(`[package-server] Zip attempt ${attempt} failed (${reason}); retrying…`)
    sleepSync(WIN32_ZIP_RETRY_DELAY_MS)
  }
}

function createZip(stagingRoot, zipPath) {
  fs.mkdirSync(path.dirname(zipPath), { recursive: true })
  if (fs.existsSync(zipPath)) fs.unlinkSync(zipPath)

  const innerDirName = 'pointer-server'

  if (process.platform === 'win32') {
    createZipWindows(stagingRoot, zipPath, innerDirName)
    return
  }

  const zipCheck = spawnSync('zip', ['-h'], { stdio: 'ignore' })
  if (zipCheck.status !== 0) {
    console.error('[package-server] `zip` command not found. Install zip (e.g. apt install zip).')
    process.exit(1)
  }

  const result = spawnSync('zip', ['-r', zipPath, innerDirName], {
    cwd: stagingRoot,
    stdio: 'inherit',
  })
  if (result.status !== 0) {
    console.error('[package-server] zip failed.')
    process.exit(result.status ?? 1)
  }
}

function main() {
  ensureInputs()

  const bundleRoot = path.join(ROOT, 'target', 'release', 'pointer-server-bundle')
  const stagingRoot = path.join(bundleRoot, 'staging')
  const bundleName = `pointer-server-${platformTag()}`
  const stagingDir = path.join(stagingRoot, 'pointer-server')
  const zipPath = path.join(bundleRoot, `${bundleName}.zip`)

  fs.rmSync(stagingRoot, { recursive: true, force: true })
  fs.mkdirSync(stagingDir, { recursive: true })

  fs.copyFileSync(BIN_PATH, path.join(stagingDir, BIN_NAME))
  copyTree(DIST_DIR, path.join(stagingDir, 'dist'))
  copyTree(SKILLS_DIR, path.join(stagingDir, 'skills'))
  fs.copyFileSync(CONFIG_EXAMPLE, path.join(stagingDir, 'pointer-server.toml.example'))
  syncServerDeployScripts(stagingDir)

  const releaseDir = path.join(ROOT, 'target', 'release')
  syncServerDeployScripts(releaseDir)
  copyTree(SKILLS_DIR, path.join(releaseDir, 'skills'))

  // Defence in depth: the staging tree is built from explicit inputs (never the
  // repository root), but no release may carry per-developer or signing material.
  assertNoForbiddenReleasePaths(
    fs.readdirSync(stagingRoot, { recursive: true }),
    'server bundle staging tree'
  )

  createZip(stagingRoot, zipPath)
  fs.rmSync(stagingRoot, { recursive: true, force: true })

  const sizeMb = (fs.statSync(zipPath).size / (1024 * 1024)).toFixed(1)
  console.log(`[package-server] Created ${zipPath} (${sizeMb} MiB)`)
  console.log('[package-server] Bundle layout (inside pointer-server/):')
  console.log(`  pointer-server/${BIN_NAME}`)
  console.log('  pointer-server/dist/')
  console.log('  pointer-server/skills/          (default bundled skills)')
  console.log('  pointer-server/pointer-server.toml.example')
  console.log('  pointer-server/start.sh / stop.sh / restart.sh / status.sh')
  console.log('  pointer-server/start.ps1 / stop.ps1 / restart.ps1 / status.ps1 (Windows)')
  console.log('[package-server] Deploy: unzip, cd pointer-server, copy pointer-server.toml.example → pointer-server.toml, then ./start.sh or .\\start.ps1')
}

main()
