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

function createZip(stagingRoot, zipPath) {
  fs.mkdirSync(path.dirname(zipPath), { recursive: true })
  if (fs.existsSync(zipPath)) fs.unlinkSync(zipPath)

  const innerDirName = 'license-gen'
  const innerPath = path.join(stagingRoot, innerDirName)

  if (process.platform === 'win32') {
    const ps = [
      '-NoProfile',
      '-Command',
      `Compress-Archive -Path '${innerPath.replace(/'/g, "''")}' -DestinationPath '${zipPath.replace(/'/g, "''")}' -Force`,
    ]
    const result = spawnSync('powershell.exe', ps, { stdio: 'inherit' })
    if (result.status !== 0) {
      console.error('[license-gen] Compress-Archive failed.')
      process.exit(result.status ?? 1)
    }
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
See docs/developer/standalone-deployment.md for full details.
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
