#!/usr/bin/env node
/**
 * Cross-platform pointer-admin (control plane) build — the admin sibling of
 * `scripts/build-server.mjs`.
 *
 *   npm run admin:build
 *     → admin/web Vue build + cargo release + assembled bundle + tar.gz
 *
 *   npm run admin:package   # bundle only, no compile
 *
 * Why a script at all: the control plane ships as **two halves** — the
 * `pointer-admin` binary and the `admin/web/dist` static assets it serves via
 * `POINTER_ADMIN_STATIC_DIR`. Build them separately and you can easily end up
 * serving yesterday's UI from today's binary, with no error to point at it.
 * `server:build` solves the same problem with `verify-baked-edition.mjs`; here
 * the halves carry no edition, so the equivalent guard is: build both in one
 * invocation, then record a manifest (commit + sha256 of each half) inside the
 * bundle so a mismatched pair is detectable after the fact.
 *
 * The bundle is a directory plus a `.tar.gz`:
 *
 *   target/release/pointer-admin-bundle/pointer-admin-{os}-{arch}/
 *     ├── pointer-admin          (or pointer-admin.exe)
 *     ├── web/                   (copied from admin/web/dist)
 *     ├── build-info.json        (commit, builtAt, sha256 of both halves)
 *     └── README.txt             (how to run it)
 */
import { spawnSync } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { platformLabel, platformTag } from './lib/platform-tag.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '..')
const args = process.argv.slice(2)
const packageOnly = args.includes('--package-only')

const BIN_NAME = process.platform === 'win32' ? 'pointer-admin.exe' : 'pointer-admin'
const BIN_PATH = path.join(ROOT, 'target', 'release', BIN_NAME)
const WEB_DIR = path.join(ROOT, 'admin', 'web')
const DIST_DIR = path.join(WEB_DIR, 'dist')
const BUNDLE_ROOT = path.join(ROOT, 'target', 'release', 'pointer-admin-bundle')
const BUNDLE_NAME = `pointer-admin-${platformTag()}`
const BUNDLE_DIR = path.join(BUNDLE_ROOT, BUNDLE_NAME)

const log = (msg) => console.log(`[admin-build] ${msg}`)
const warn = (msg) => console.warn(`[admin-build] WARNING: ${msg}`)

function fail(msg, hints = []) {
  console.error(`[admin-build] ${msg}`)
  for (const hint of hints) console.error(`  • ${hint}`)
  process.exit(1)
}

function run(command, commandArgs, { label } = {}) {
  if (label) log(label)
  const result = spawnSync(command, commandArgs, {
    cwd: ROOT,
    stdio: 'inherit',
    shell: false,
  })
  if (result.error) fail(`无法执行 ${command}: ${result.error.message}`)
  if (result.status !== 0) {
    fail(`${command} 退出码 ${result.status}`, ['修好上面的报错后重新运行。'])
  }
}

function npmCommand() {
  return process.platform === 'win32' ? 'npm.cmd' : 'npm'
}

function buildWeb() {
  if (!fs.existsSync(path.join(WEB_DIR, 'package.json'))) {
    fail(`找不到 ${path.relative(ROOT, WEB_DIR)}/package.json`)
  }
  run(npmCommand(), ['--prefix', 'admin/web', 'run', 'build'], {
    label: '前端：npm --prefix admin/web run build',
  })
}

function buildBinary() {
  run('cargo', ['build', '-p', 'pointer-admin', '--release'], {
    label: '二进制：cargo build -p pointer-admin --release',
  })
}

/** Newest mtime under `dir`, or 0 when it does not exist. */
function newestMtime(dir) {
  let newest = 0
  const walk = (current) => {
    let entries
    try {
      entries = fs.readdirSync(current, { withFileTypes: true })
    } catch {
      return
    }
    for (const entry of entries) {
      const full = path.join(current, entry.name)
      if (entry.isDirectory()) {
        walk(full)
      } else {
        try {
          newest = Math.max(newest, fs.statSync(full).mtimeMs)
        } catch {
          /* raced with a delete — ignore */
        }
      }
    }
  }
  walk(dir)
  return newest
}

function sha256File(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
}

function gitCommit() {
  const result = spawnSync('git', ['rev-parse', 'HEAD'], {
    cwd: ROOT,
    encoding: 'utf8',
  })
  if (result.status !== 0) return null
  return (result.stdout || '').trim() || null
}

function countFiles(dir) {
  let total = 0
  const walk = (current) => {
    let entries
    try {
      entries = fs.readdirSync(current, { withFileTypes: true })
    } catch {
      return
    }
    for (const entry of entries) {
      if (entry.isDirectory()) walk(path.join(current, entry.name))
      else total += 1
    }
  }
  walk(dir)
  return total
}

/**
 * Both halves must exist and be plausible. Staleness is a warning, not a
 * failure: mtimes are not authoritative after a checkout or an rsync, and a
 * hard failure there would block legitimate rebuilds.
 */
function verifyArtifacts() {
  if (!fs.existsSync(BIN_PATH)) {
    fail('缺少二进制，控制面无法打包。', [
      `期望位置：${path.relative(ROOT, BIN_PATH)}`,
      '先运行 npm run admin:build（或 cargo build -p pointer-admin --release）。',
    ])
  }
  const indexHtml = path.join(DIST_DIR, 'index.html')
  if (!fs.existsSync(indexHtml)) {
    fail('缺少前端产物，控制面起来会白屏。', [
      `期望位置：${path.relative(ROOT, indexHtml)}`,
      '先运行 npm run admin:build（或 npm --prefix admin/web run build）。',
    ])
  }

  const webNewest = newestMtime(path.join(WEB_DIR, 'src'))
  const distTime = fs.statSync(indexHtml).mtimeMs
  if (webNewest > distTime) {
    warn(
      'admin/web/src 比 dist 新，前端产物可能过期。' +
        '重新运行 npm --prefix admin/web run build 再打包。',
    )
  }

  const srcNewest = newestMtime(path.join(ROOT, 'admin', 'src'))
  const binTime = fs.statSync(BIN_PATH).mtimeMs
  if (srcNewest > binTime) {
    warn('admin/src 比二进制新，二进制可能过期。重新 cargo build 再打包。')
  }
}

function assembleBundle() {
  fs.rmSync(BUNDLE_DIR, { recursive: true, force: true })
  fs.mkdirSync(BUNDLE_DIR, { recursive: true })

  const targetBinary = path.join(BUNDLE_DIR, BIN_NAME)
  fs.copyFileSync(BIN_PATH, targetBinary)
  if (process.platform !== 'win32') fs.chmodSync(targetBinary, 0o755)

  fs.cpSync(DIST_DIR, path.join(BUNDLE_DIR, 'web'), { recursive: true })

  // 生产用的启停脚本：纯 sh，不依赖 node，解包即可用。
  const ctlSource = path.join(__dirname, 'pointer-admin-ctl.sh')
  const ctlTarget = path.join(BUNDLE_DIR, 'pointer-admin.sh')
  fs.copyFileSync(ctlSource, ctlTarget)
  fs.chmodSync(ctlTarget, 0o755)

  const info = {
    product: 'pointer-admin',
    platform: platformTag(),
    commit: gitCommit(),
    builtAt: new Date().toISOString(),
    binary: {
      name: BIN_NAME,
      bytes: fs.statSync(BIN_PATH).size,
      sha256: sha256File(BIN_PATH),
    },
    web: {
      files: countFiles(DIST_DIR),
      indexSha256: sha256File(path.join(DIST_DIR, 'index.html')),
    },
  }
  fs.writeFileSync(
    path.join(BUNDLE_DIR, 'build-info.json'),
    `${JSON.stringify(info, null, 2)}\n`,
    'utf8',
  )

  fs.writeFileSync(
    path.join(BUNDLE_DIR, 'README.txt'),
    [
      'Pointer 控制面（管理端）',
      '',
      '启动（生产）：',
      '  ./pointer-admin.sh start     后台启动，写 PID 文件',
      '  ./pointer-admin.sh status    运行中退出 0，已停止退出 1',
      '  ./pointer-admin.sh stop',
      '  ./pointer-admin.sh restart',
      '',
      '不依赖 node / npm。也可以直接前台运行：',
      `  POINTER_ADMIN_STATIC_DIR=./web ./${BIN_NAME}`,
      '',
      '必填环境变量：',
      '  POINTER_ADMIN_OAUTH_SECRET      客户端换票用，必须与客户端构建时一致',
      '  POINTER_ADMIN_BOOTSTRAP_USER    首次启动建号，建好可去掉',
      '  POINTER_ADMIN_BOOTSTRAP_PASSWORD',
      '',
      '常用可选：',
      '  POINTER_ADMIN_ADDR              默认 127.0.0.1:8788',
      '  POINTER_ADMIN_DB_URL            默认 sqlite:pointer-admin.db',
      '  POINTER_ADMIN_DATA_DIR          上传的安装包等运行时数据',
      '  POINTER_ADMIN_PUBLIC_BASE       对外访问地址',
      '',
      '完整步骤见 docs/zh-CN/deploy/enterprise.md（英文：docs/en/deploy/enterprise.md）。',
      'build-info.json 记录了两半的 commit 与 sha256，排查版本不一致时先看它。',
      '',
    ].join('\n'),
    'utf8',
  )

  const archive = path.join(BUNDLE_ROOT, `${BUNDLE_NAME}.tar.gz`)
  fs.rmSync(archive, { force: true })
  run('tar', ['-czf', archive, '-C', BUNDLE_ROOT, BUNDLE_NAME])
  return archive
}

function printSummary(archive) {
  console.log('')
  log(`完成（${platformLabel()}）`)
  log(`目录：${path.relative(ROOT, BUNDLE_DIR)}`)
  if (archive && fs.existsSync(archive)) {
    const mb = (fs.statSync(archive).size / (1024 * 1024)).toFixed(1)
    log(`归档：${path.relative(ROOT, archive)} (${mb} MiB)`)
  }
  log('部署：解包后设 POINTER_ADMIN_STATIC_DIR=./web 再启动二进制。')
}

function main() {
  log(`平台：${platformLabel()}`)

  if (!packageOnly) {
    buildWeb()
    buildBinary()
  } else {
    log('--package-only：跳过构建，直接打包现有产物')
  }

  verifyArtifacts()
  const archive = assembleBundle()
  printSummary(archive)
}

main()
