#!/usr/bin/env node
/**
 * Cross-platform pointer-server build (mirrors scripts/tauri-build.mjs).
 *
 *   npm run server:build
 *     → same-origin Vue + cargo release + zip on all platforms
 *     → additionally .deb on Linux when dpkg-deb is available
 *
 *   npm run server:build -- --package-only   # zip only, no compile
 *   npm run server:build -- --deb-only       # .deb only (Linux, inputs must exist)
 */
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { platformLabel, platformTag } from './lib/platform-tag.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '..')
const args = process.argv.slice(2)
const packageOnly = args.includes('--package-only')
const debOnly = args.includes('--deb-only')

function runNodeScript(scriptName, scriptArgs = []) {
  const scriptPath = path.join(__dirname, scriptName)
  const result = spawnSync(process.execPath, [scriptPath, ...scriptArgs], {
    cwd: ROOT,
    stdio: 'inherit',
  })
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

function runCargoBuild() {
  console.log('[server-build] cargo build -p pointer-server --release')
  const result = spawnSync('cargo', ['build', '-p', 'pointer-server', '--release'], {
    cwd: ROOT,
    stdio: 'inherit',
  })
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

function dpkgDebAvailable() {
  if (process.platform !== 'linux') return false
  const check = spawnSync('dpkg-deb', ['--version'], { stdio: 'ignore' })
  return check.status === 0
}

function maybeBuildDeb() {
  if (process.platform !== 'linux') {
    console.log('[server-build] Skipping .deb (Linux only). Zip bundle is ready.')
    return
  }
  if (!dpkgDebAvailable()) {
    console.warn('[server-build] Skipping .deb: dpkg-deb not found (sudo apt install dpkg).')
    return
  }
  console.log('[server-build] Linux: building .deb package…')
  runNodeScript('build-server-deb.mjs')
}

function printSummary() {
  const zipPath = path.join(
    ROOT,
    'target',
    'release',
    'pointer-server-bundle',
    `pointer-server-${platformTag()}.zip`
  )
  const debDir = path.join(ROOT, 'target', 'release', 'bundle', 'deb')
  console.log('')
  console.log(`[server-build] Done (${platformLabel()}).`)
  if (fs.existsSync(zipPath)) {
    const mb = (fs.statSync(zipPath).size / (1024 * 1024)).toFixed(1)
    console.log(`[server-build] Zip:  ${zipPath} (${mb} MiB)`)
  }
  if (fs.existsSync(debDir)) {
    const debs = fs.readdirSync(debDir).filter(f => f.endsWith('.deb'))
    for (const deb of debs) {
      const p = path.join(debDir, deb)
      const kb = (fs.statSync(p).size / 1024).toFixed(0)
      console.log(`[server-build] Deb:  ${p} (${kb} KiB)`)
    }
  }
}

function main() {
  console.log(`[server-build] Platform: ${platformLabel()}`)

  if (debOnly) {
    maybeBuildDeb()
    printSummary()
    return
  }

  if (!packageOnly) {
    runNodeScript('build-web-for-server.mjs')
    runCargoBuild()
  }

  runNodeScript('package-server.mjs')
  maybeBuildDeb()
  printSummary()
}

main()
