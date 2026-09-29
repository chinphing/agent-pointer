#!/usr/bin/env node
/**
 * Build pointer-server .deb package.
 *
 * Prerequisites:
 *   npm run build              (Vue frontend → dist/)
 *   cargo build -p pointer-server --release
 *   dpkg-deb (included in dpkg package)
 *
 * Usage:
 *   npm run server:build:deb   (Linux only)
 *   npm run server:deb         (build frontend + binary + .deb)
 */
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { assertNoForbiddenReleasePaths } from './lib/release-hygiene.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '..')
const BIN_NAME = 'pointer-server'
const BIN_PATH = path.join(ROOT, 'target', 'release', BIN_NAME)
const DIST_DIR = path.join(ROOT, 'dist')
const SKILLS_DIR = path.join(ROOT, 'skills')
const CONFIG_EXAMPLE = path.join(ROOT, 'server', 'pointer-server.toml.example')
const DEB_STAGING = path.join(ROOT, 'target', 'pointer-server-deb-staging')
const PACKAGE_VERSION = JSON.parse(
  fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8')
).version
const PACKAGE_ARCH = 'amd64'

function ensureInputs() {
  const missing = []
  if (!fs.existsSync(BIN_PATH)) missing.push(BIN_PATH)
  if (!fs.existsSync(DIST_DIR)) missing.push(DIST_DIR)
  if (!fs.existsSync(SKILLS_DIR)) missing.push(SKILLS_DIR)
  if (!fs.existsSync(CONFIG_EXAMPLE)) missing.push(CONFIG_EXAMPLE)
  if (missing.length > 0) {
    console.error('[build-deb] Missing required inputs:')
    for (const item of missing) console.error(`  • ${item}`)
    console.error('\nRun first: npm run build && cargo build -p pointer-server --release')
    process.exit(1)
  }
}

function rmTree(dir) {
  fs.rmSync(dir, { recursive: true, force: true })
}

function mkdir(p) {
  fs.mkdirSync(p, { recursive: true })
}

function copyTree(src, dest) {
  fs.cpSync(src, dest, { recursive: true })
}

function writeFile(p, content) {
  mkdir(path.dirname(p))
  fs.writeFileSync(p, content, 'utf-8')
}

function chmod(p, mode) {
  fs.chmodSync(p, mode)
}

function ensureLinuxDebTooling() {
  if (process.platform !== 'linux') {
    console.error('[build-deb] .deb packaging requires Linux (dpkg-deb).')
    console.error('[build-deb] Current platform:', process.platform)
    console.error('[build-deb] On macOS/Windows use: npm run server:build  → zip bundle')
    console.error('[build-deb] On Linux CI/host use:   npm run server:deb    → .deb package')
    process.exit(1)
  }
  const debCheck = spawnSync('dpkg-deb', ['--version'], { stdio: 'ignore' })
  if (debCheck.status !== 0) {
    console.error('[build-deb] dpkg-deb not found. Install: sudo apt install dpkg')
    process.exit(1)
  }
}

function main() {
  ensureLinuxDebTooling()
  ensureInputs()

  // Detect architecture from binary
  const fileCheck = spawnSync('file', [BIN_PATH], { stdio: ['pipe', 'pipe', 'pipe'] })
  const fileOut = fileCheck.stdout.toString()
  const arch = fileOut.includes('aarch64') || fileOut.includes('arm64')
    ? 'arm64'
    : fileOut.includes('x86-64')
      ? 'amd64'
      : PACKAGE_ARCH
  const debArch = arch

  // Clean staging
  rmTree(DEB_STAGING)

  // Build directory layout
  // DEBIAN/         → control, postinst, prerm
  // usr/bin/        → pointer-server
  // usr/share/pointer-server/dist/  → Vue frontend
  // usr/share/pointer-server/skills/ → bundled skills
  // etc/pointer-server/ → pointer-server.toml.example
  // lib/systemd/system/ → pointer-server.service

  const D = DEB_STAGING
  const PKG_NAME = `pointer-server_${PACKAGE_VERSION}_${debArch}`

  // Binary
  const binDir = path.join(D, 'usr', 'bin')
  mkdir(binDir)
  fs.copyFileSync(BIN_PATH, path.join(binDir, BIN_NAME))
  chmod(path.join(binDir, BIN_NAME), 0o755)

  // Vue dist
  copyTree(DIST_DIR, path.join(D, 'usr', 'share', 'pointer-server', 'dist'))

  // Skills
  copyTree(SKILLS_DIR, path.join(D, 'usr', 'share', 'pointer-server', 'skills'))

  // Config example (not live config)
  const etcDir = path.join(D, 'etc', 'pointer-server')
  mkdir(etcDir)
  fs.copyFileSync(CONFIG_EXAMPLE, path.join(etcDir, 'pointer-server.toml.example'))

  // Systemd service
  const systemdDir = path.join(D, 'lib', 'systemd', 'system')
  mkdir(systemdDir)
  writeFile(
    path.join(systemdDir, 'pointer-server.service'),
    `[Unit]
Description=Pointer Server — AI Agent Service
After=network.target
Wants=network.target

[Service]
Type=simple
ExecStart=/usr/bin/pointer-server
Restart=on-failure
RestartSec=5
Environment=POINTER_DEPLOYMENT_MODE=standalone
Environment=POINTER_APP_DATA_DIR=/var/lib/pointer-server
Environment=POINTER_SERVER_CONFIG=/etc/pointer-server/pointer-server.toml
Environment=POINTER_SERVER_STATIC_DIR=/usr/share/pointer-server/dist
Environment=POINTER_SERVER_SKILLS_DIR=/usr/share/pointer-server/skills

# Security
NoNewPrivileges=yes
PrivateDevices=yes
ProtectSystem=full
ProtectHome=yes

[Install]
WantedBy=multi-user.target
`
  )

  // DEBIAN control
  const debianDir = path.join(D, 'DEBIAN')
  mkdir(debianDir)
  writeFile(
    path.join(debianDir, 'control'),
    `Package: pointer-server
Version: ${PACKAGE_VERSION}
Section: net
Priority: optional
Architecture: ${debArch}
Maintainer: Pointer Team <dev@readflowai.com>
Depends: libc6 (>= 2.31)
Description: Pointer Server — Standalone AI Agent Service
 Pointer Server is a standalone AI agent backend service.
 It provides LLM-powered chat, webhook automation, task management,
 and IM channel integration. This package is the server-only build
 for self-hosted / independent deployments.
 .
 Key features:
  • Full AI agent runtime (chat, task board, skill system)
  • Standalone mode (no cloud dependency)
  • Ed25519 license enforcement
  • Admin token authentication
  • Webhook and cron job automation
  • IM channel integration (WeChat, Feishu, DingTalk, WeCom)
`
  )

  // postinst
  writeFile(
    path.join(debianDir, 'postinst'),
    `#!/bin/sh
set -e

case "$1" in
  configure)
    # Create data directory
    mkdir -p /var/lib/pointer-server
    chmod 755 /var/lib/pointer-server

    # Create log directory (used by default)
    mkdir -p /var/log/pointer-server
    chmod 755 /var/log/pointer-server

    echo "Pointer Server data directory: /var/lib/pointer-server"
    echo "Pointer Server log directory:  /var/log/pointer-server"
    echo ""
    echo "To configure standalone mode:"
    echo "  cp /etc/pointer-server/pointer-server.toml.example /etc/pointer-server/pointer-server.toml"
    echo "  # Edit /etc/pointer-server/pointer-server.toml with your settings"
    echo "  systemctl enable --now pointer-server"
    echo ""
    echo "To get this machine's ID for license binding:"
    echo "  /usr/bin/pointer-server --machine-id"
    ;;
esac
`
  )
  chmod(path.join(debianDir, 'postinst'), 0o755)

  // prerm
  writeFile(
    path.join(debianDir, 'prerm'),
    `#!/bin/sh
set -e

case "$1" in
  remove|purge)
    if command -v systemctl >/dev/null 2>&1; then
      systemctl stop pointer-server 2>/dev/null || true
      systemctl disable pointer-server 2>/dev/null || true
    fi
    ;;
esac
`
  )
  chmod(path.join(debianDir, 'prerm'), 0o755)

  // Defence in depth: the staging tree is built from explicit inputs (never the
  // repository root), but no release may carry per-developer or signing material.
  assertNoForbiddenReleasePaths(
    fs.readdirSync(D, { recursive: true }),
    'deb staging tree'
  )

  // Build .deb
  const debPath = path.join(ROOT, 'target', 'release', 'bundle', 'deb')
  mkdir(debPath)

  const result = spawnSync('dpkg-deb', ['--build', D, path.join(debPath, `${PKG_NAME}.deb`)], {
    stdio: 'inherit',
  })

  if (result.status !== 0) {
    console.error('[build-deb] dpkg-deb failed.')
    process.exit(result.status ?? 1)
  }

  // Cleanup staging
  rmTree(DEB_STAGING)

  // Verify
  const finalPath = path.join(debPath, `${PKG_NAME}.deb`)
  const sizeKb = (fs.statSync(finalPath).size / 1024).toFixed(0)
  console.log(`[build-deb] Created: ${finalPath} (${sizeKb} KiB)`)
  console.log(`[build-deb] Architecture: ${debArch}`)
  console.log('')
  console.log('Install:')
  console.log(`  sudo dpkg -i ${finalPath}`)
  console.log('  sudo systemctl enable --now pointer-server')
  console.log('')
  console.log('First-time setup:')
  console.log('  # Get machine ID for license')
  console.log('  /usr/bin/pointer-server --machine-id')
  console.log('')
  console.log('  # Configure')
  console.log('  sudo cp /etc/pointer-server/pointer-server.toml.example /etc/pointer-server/pointer-server.toml')
  console.log('  sudo vi /etc/pointer-server/pointer-server.toml')
  console.log('  sudo systemctl restart pointer-server')
}

main()
