#!/usr/bin/env node
/**
 * Cross-platform pointer-server build — the server-side sibling of
 * `scripts/tauri-build.mjs` (same `{module}:dev|build` family, different product).
 *
 *   npm run server:build
 *     → same-origin Vue + cargo release + zip on all platforms
 *     → additionally .deb on Linux when dpkg-deb is available
 *
 *   npm run server:build -- --package-only   # zip only, no compile
 *   npm run server:build -- --deb-only       # .deb only (Linux, inputs must exist)
 *
 * Reads the gitignored `pointer.local.env` (already-set OS / CI env still wins)
 * and hands the result to both halves — the Vue build and cargo — so one file
 * binds the whole artifact. Afterwards the two halves are compared: a managed
 * build whose binary or web assets are missing the control plane exits non-zero
 * instead of shipping a half-bound release. See scripts/lib/verify-baked-edition.mjs.
 */
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { platformLabel, platformTag } from './lib/platform-tag.mjs'
import { applyPointerLocalEnv } from './lib/load-pointer-local-env.mjs'
import { verifyBuildArtifacts } from './lib/verify-baked-edition.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '..')
const args = process.argv.slice(2)
const packageOnly = args.includes('--package-only')
const debOnly = args.includes('--deb-only')

const BIN_NAME = process.platform === 'win32' ? 'pointer-server.exe' : 'pointer-server'
const BIN_PATH = path.join(ROOT, 'target', 'release', BIN_NAME)
const DIST_DIR = path.join(ROOT, 'dist')

/** OS / CI env wins; `pointer.local.env` only fills the blanks (VITE_* mirrored). */
const env = { ...process.env }
applyPointerLocalEnv(env, { logPrefix: '[server-build]' })

function runNodeScript(scriptName, scriptArgs = []) {
  const scriptPath = path.join(__dirname, scriptName)
  const result = spawnSync(process.execPath, [scriptPath, ...scriptArgs], {
    cwd: ROOT,
    env,
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
    env,
    stdio: 'inherit',
  })
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

/**
 * Fail closed on a half-bound artifact: a `managed` build must carry the
 * control plane in *both* halves. Standalone builds only report what they baked.
 * Runs for real builds and for `--package-only` / `--deb-only` (existing artifacts).
 */
function verifyEditionOrExit() {
  const result = verifyBuildArtifacts({ env, binaryPath: BIN_PATH, webDir: DIST_DIR })
  console.log(`[server-build] ${result.summary}`)
  for (const note of result.notes) {
    console.log(`[server-build]   · ${note}`)
  }
  for (const warning of result.warnings) {
    console.warn(`[server-build] WARNING: ${warning}`)
  }
  if (result.ok) return
  console.error('[server-build] Build aborted — the release would be half-bound:')
  for (const error of result.errors) {
    console.error(`  • ${error}`)
  }
  console.error(
    '[server-build] Fix: set POINTER_EDITION=managed plus POINTER_API_BASE / POINTER_WEB_BASE / ' +
      'COMPUTER_ANNOTATE_API_BASE in pointer.local.env (or the OS / CI env), then rebuild both halves. ' +
      'See docs/en/deploy/editions.md.',
  )
  process.exit(1)
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
    verifyEditionOrExit()
    maybeBuildDeb()
    printSummary()
    return
  }

  if (!packageOnly) {
    runNodeScript('build-web-for-server.mjs')
    runCargoBuild()
  }

  // Before packaging: never zip a half-bound artifact.
  verifyEditionOrExit()
  runNodeScript('package-server.mjs')
  maybeBuildDeb()
  printSummary()
}

main()
