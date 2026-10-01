#!/usr/bin/env node
/**
 * Cross-platform license-gen build (mirrors scripts/tauri-build.mjs / build-server.mjs).
 *
 *   npm run license-gen:build
 *     → cargo release + platform zip on all platforms
 *
 *   npm run license-gen:build -- --bin-only   # compile only, no zip
 */
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { platformLabel, platformTag } from './lib/platform-tag.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '..')
const BIN_NAME = process.platform === 'win32' ? 'license-gen.exe' : 'license-gen'
const BIN_PATH = path.join(ROOT, 'target', 'release', BIN_NAME)
const binOnly = process.argv.includes('--bin-only')

function runCargoBuild() {
  console.log('[license-gen] cargo build -p pointer-license-gen --release')
  const result = spawnSync('cargo', ['build', '-p', 'pointer-license-gen', '--release'], {
    cwd: ROOT,
    stdio: 'inherit',
  })
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
  if (!fs.existsSync(BIN_PATH)) {
    console.error(`[license-gen] Expected binary missing: ${BIN_PATH}`)
    process.exit(1)
  }
  const sizeMb = (fs.statSync(BIN_PATH).size / (1024 * 1024)).toFixed(1)
  console.log(`[license-gen] Built ${BIN_PATH} (${sizeMb} MiB)`)
}

/** Antivirus scanners hold a freshly copied binary for a few seconds — retry past that. */
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
 * powershell.exe still exits 0 and the bundle silently ships without the binary
 * (antivirus scanning it right after the copy). bsdtar exits non-zero on a locked input,
 * and the finished archive is verified afterwards — a partial zip is deleted, never kept.
 */
function createZipWindows(stagingRoot, zipPath, innerDirName) {
  const tarCheck = spawnSync('tar', ['--version'], { stdio: 'ignore' })
  if (tarCheck.status !== 0) {
    console.error(
      '[license-gen] `tar` not found. Windows 10 1803+ bundles C:\\Windows\\System32\\tar.exe.'
    )
    process.exit(1)
  }

  const requiredEntries = [`${innerDirName}/${BIN_NAME}`, `${innerDirName}/README.txt`]
  for (let attempt = 1; attempt <= WIN32_ZIP_ATTEMPTS; attempt += 1) {
    const result = spawnSync('tar', ['-a', '-c', '-f', zipPath, '-C', stagingRoot, innerDirName], {
      stdio: 'inherit',
    })
    const entries = result.status === 0 ? listZipEntries(zipPath) : null
    const missing =
      entries === null ? null : requiredEntries.filter(entry => !entries.includes(entry))
    if (missing !== null && missing.length === 0) return

    const reason =
      result.status !== 0
        ? `tar exited with code ${result.status ?? 'null'}`
        : `archive is missing ${missing.join(', ')}`
    fs.rmSync(zipPath, { force: true })
    if (attempt === WIN32_ZIP_ATTEMPTS) {
      console.error(
        `[license-gen] Zip creation failed (${reason}) after ${WIN32_ZIP_ATTEMPTS} attempts.`
      )
      console.error('[license-gen] No zip was kept — check the locked file and re-run the build.')
      process.exit(1)
    }
    console.warn(`[license-gen] Zip attempt ${attempt} failed (${reason}); retrying…`)
    sleepSync(WIN32_ZIP_RETRY_DELAY_MS)
  }
}

function createZip(stagingRoot, zipPath) {
  fs.mkdirSync(path.dirname(zipPath), { recursive: true })
  if (fs.existsSync(zipPath)) fs.unlinkSync(zipPath)

  const innerDirName = 'license-gen'

  if (process.platform === 'win32') {
    createZipWindows(stagingRoot, zipPath, innerDirName)
    return
  }

  const zipCheck = spawnSync('zip', ['-h'], { stdio: 'ignore' })
  if (zipCheck.status !== 0) {
    console.error('[license-gen] `zip` command not found. Install zip (e.g. apt install zip).')
    process.exit(1)
  }

  const result = spawnSync('zip', ['-r', zipPath, innerDirName], {
    cwd: stagingRoot,
    stdio: 'inherit',
  })
  if (result.status !== 0) {
    console.error('[license-gen] zip failed.')
    process.exit(result.status ?? 1)
  }
}

function packageRelease() {
  const readme = `# license-gen

Offline Ed25519 license signing tool for pointer-server standalone deployments.

## Usage

\`\`\`bash
./${BIN_NAME} gen-keypair --private-key license.key --public-key license.pub
./${BIN_NAME} sign --private-key license.key --customer-id acme --expires 2027-12-31 --features chat,webhook
\`\`\`

Keep \`license.key\` offline. Embed \`license.pub\` in pointer-core/license.pub for release builds.
See docs/zh-CN/developer/standalone-deployment.md for full details.
`

  const bundleRoot = path.join(ROOT, 'target', 'release', 'license-gen-bundle')
  const stagingRoot = path.join(bundleRoot, 'staging')
  const bundleName = `license-gen-${platformTag()}`
  const stagingDir = path.join(stagingRoot, 'license-gen')
  const zipPath = path.join(bundleRoot, `${bundleName}.zip`)

  fs.rmSync(stagingRoot, { recursive: true, force: true })
  fs.mkdirSync(stagingDir, { recursive: true })

  fs.copyFileSync(BIN_PATH, path.join(stagingDir, BIN_NAME))
  fs.writeFileSync(path.join(stagingDir, 'README.txt'), readme, 'utf8')

  createZip(stagingRoot, zipPath)
  fs.rmSync(stagingRoot, { recursive: true, force: true })

  const sizeMb = (fs.statSync(zipPath).size / (1024 * 1024)).toFixed(1)
  console.log(`[license-gen] Created ${zipPath} (${sizeMb} MiB)`)
  return zipPath
}

function main() {
  console.log(`[license-gen] Platform: ${platformLabel()}`)
  runCargoBuild()
  if (!binOnly) {
    packageRelease()
  }
  console.log('[license-gen] Done.')
}

main()
