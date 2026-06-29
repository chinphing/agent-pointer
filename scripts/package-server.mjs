#!/usr/bin/env node
/**
 * Package pointer-server release binary, dist/, and config example into a zip.
 * Invoked automatically at the end of `npm run server:build`.
 */
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { syncServerDeployScripts } from './lib/sync-server-deploy-scripts.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '..')
const BIN_NAME = process.platform === 'win32' ? 'pointer-server.exe' : 'pointer-server'
const BIN_PATH = path.join(ROOT, 'target', 'release', BIN_NAME)
const DIST_DIR = path.join(ROOT, 'dist')
const SKILLS_DIR = path.join(ROOT, 'skills')
const CONFIG_EXAMPLE = path.join(ROOT, 'server', 'pointer-server.toml.example')

function platformTag() {
  const osName =
    process.platform === 'darwin'
      ? 'macos'
      : process.platform === 'win32'
        ? 'windows'
        : process.platform
  const arch = process.arch === 'arm64' ? 'arm64' : 'x64'
  return `${osName}-${arch}`
}

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

function createZip(sourceDir, zipPath) {
  fs.mkdirSync(path.dirname(zipPath), { recursive: true })
  if (fs.existsSync(zipPath)) fs.unlinkSync(zipPath)

  if (process.platform === 'win32') {
    const ps = [
      '-NoProfile',
      '-Command',
      `Compress-Archive -Path '${sourceDir.replace(/'/g, "''")}\\*' -DestinationPath '${zipPath.replace(/'/g, "''")}' -Force`,
    ]
    const result = spawnSync('powershell.exe', ps, { stdio: 'inherit' })
    if (result.status !== 0) {
      console.error('[package-server] Compress-Archive failed.')
      process.exit(result.status ?? 1)
    }
    return
  }

  const zipCheck = spawnSync('zip', ['-h'], { stdio: 'ignore' })
  if (zipCheck.status !== 0) {
    console.error('[package-server] `zip` command not found. Install zip (e.g. apt install zip).')
    process.exit(1)
  }

  const result = spawnSync('zip', ['-r', zipPath, '.'], {
    cwd: sourceDir,
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
  const stagingDir = path.join(stagingRoot, bundleName)
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

  createZip(stagingDir, zipPath)
  fs.rmSync(stagingRoot, { recursive: true, force: true })

  const sizeMb = (fs.statSync(zipPath).size / (1024 * 1024)).toFixed(1)
  console.log(`[package-server] Created ${zipPath} (${sizeMb} MiB)`)
  console.log('[package-server] Bundle layout:')
  console.log(`  ${BIN_NAME}`)
  console.log('  dist/')
  console.log('  skills/          (default bundled skills)')
  console.log('  pointer-server.toml.example')
  console.log('  start.sh / stop.sh / restart.sh / status.sh')
  console.log('  start.ps1 / stop.ps1 / restart.ps1 / status.ps1 (Windows)')
  console.log('[package-server] Deploy: unzip, copy pointer-server.toml.example → pointer-server.toml, then ./start.sh or .\\start.ps1')
}

main()
