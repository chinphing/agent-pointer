#!/usr/bin/env node
/**
 * Regenerate THIRD-PARTY-NOTICES.md from the JavaScript production tree and the
 * Rust dependency tree.
 *
 * Run after dependency changes:
 *   npm run licenses
 *
 * The file lists every third-party package we redistribute inside a build, plus
 * the full text of each distinct license found next to those packages. Bundled
 * MIT/BSD/ISC code requires its copyright + permission notice to travel with the
 * build; Apache-2.0 requires the license text and attribution notices.
 */
import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const outFile = path.join(root, 'THIRD-PARTY-NOTICES.md')

const LICENSE_FILE_RE = /^(LICEN[CS]E|COPYING)(\.(md|txt|markdown))?$/i

function readLicenseText(dir) {
  let entries
  try {
    entries = fs.readdirSync(dir)
  } catch {
    return null
  }
  for (const name of entries) {
    if (!LICENSE_FILE_RE.test(name)) continue
    const file = path.join(dir, name)
    try {
      if (!fs.statSync(file).isFile()) continue
      const text = fs.readFileSync(file, 'utf8').replace(/\r\n?/g, '\n').trim()
      if (text) return text
    } catch {
      /* unreadable license file: skip */
    }
  }
  return null
}

function normalizeRepo(value) {
  if (!value) return ''
  const url = typeof value === 'string' ? value : value.url || ''
  return url.replace(/^git\+/, '').replace(/\.git$/, '')
}

/** License id from a package's own package.json (lock entries sometimes omit it). */
function readPackageLicense(dir) {
  try {
    const pkg = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
    if (typeof pkg.license === 'string') return pkg.license
    if (pkg.license && typeof pkg.license === 'object' && pkg.license.type) return pkg.license.type
    if (Array.isArray(pkg.licenses)) return pkg.licenses.map((l) => l.type || l).join(' OR ')
  } catch {
    /* no readable package.json: leave blank */
  }
  return ''
}

/** Last-resort SPDX guess from the license text itself. */
function detectLicenseFromText(text) {
  if (!text) return ''
  const head = text.slice(0, 600)
  if (/MIT License/i.test(head) || /Permission is hereby granted, free of charge/i.test(head)) {
    return 'MIT'
  }
  if (/Apache License/i.test(head)) return 'Apache-2.0'
  if (/ISC License/i.test(head) || /Permission to use, copy, modify, and\/or distribute/i.test(head)) {
    return 'ISC'
  }
  if (/BSD 3-Clause/i.test(head)) return 'BSD-3-Clause'
  if (/BSD 2-Clause/i.test(head)) return 'BSD-2-Clause'
  return ''
}

/** JavaScript packages shipped in the app bundle (dev-only entries excluded). */
function jsPackages() {
  const lock = JSON.parse(fs.readFileSync(path.join(root, 'package-lock.json'), 'utf8'))
  const out = []
  for (const [key, meta] of Object.entries(lock.packages ?? {})) {
    if (!key.startsWith('node_modules/')) continue
    if (meta.dev === true) continue
    const dir = path.join(root, key)
    const text = readLicenseText(dir)
    const license =
      meta.license || readPackageLicense(dir) || detectLicenseFromText(text) || ''
    out.push({
      name: key.slice('node_modules/'.length),
      version: meta.version ?? '',
      license,
      repo: normalizeRepo(meta.repository),
      text,
    })
  }
  return out
}

/** Rust crates linked into the binaries (workspace members excluded). */
function rustPackages() {
  let raw
  try {
    raw = execFileSync('cargo', ['metadata', '--format-version', '1', '--locked'], {
      cwd: root,
      maxBuffer: 1 << 28,
      stdio: ['ignore', 'pipe', 'ignore'],
    })
  } catch {
    raw = execFileSync('cargo', ['metadata', '--format-version', '1'], {
      cwd: root,
      maxBuffer: 1 << 28,
      stdio: ['ignore', 'pipe', 'ignore'],
    })
  }
  const meta = JSON.parse(raw.toString())
  const members = new Set(meta.workspace_members ?? [])
  const out = []
  for (const pkg of meta.packages ?? []) {
    if (members.has(pkg.id)) continue
    if (!pkg.source) continue
    out.push({
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? '',
      repo: normalizeRepo(pkg.repository),
      text: readLicenseText(path.dirname(pkg.manifest_path)),
    })
  }
  return out
}

function renderInventory(title, packages) {
  const byLicense = new Map()
  for (const pkg of packages) {
    const key = pkg.license || 'UNKNOWN'
    if (!byLicense.has(key)) byLicense.set(key, [])
    byLicense.get(key).push(pkg)
  }
  const lines = [`## ${title}`, '']
  for (const key of [...byLicense.keys()].sort((a, b) => a.localeCompare(b))) {
    const list = byLicense.get(key).slice().sort((a, b) => a.name.localeCompare(b.name))
    lines.push(`### ${key} — ${list.length} package${list.length === 1 ? '' : 's'}`, '')
    for (const pkg of list) {
      const version = pkg.version ? ` ${pkg.version}` : ''
      const repo = pkg.repo ? ` — ${pkg.repo}` : ''
      lines.push(`- ${pkg.name}${version}${repo}`)
    }
    lines.push('')
  }
  return lines.join('\n')
}

function renderLicenseTexts(packages) {
  const seen = new Map()
  for (const pkg of packages) {
    if (!pkg.text) continue
    const holders = seen.get(pkg.text) ?? []
    holders.push(`${pkg.name}${pkg.version ? ` ${pkg.version}` : ''}`)
    seen.set(pkg.text, holders)
  }
  const lines = ['## License texts', '', 'One copy per distinct license text found in the tree.', '']
  let index = 0
  for (const [text, holders] of seen) {
    index += 1
    const preview = holders.slice(0, 8).join(', ')
    const more = holders.length > 8 ? `, … (+${holders.length - 8} more)` : ''
    lines.push(`### ${index}. Used by ${preview}${more}`, '', '```text', text, '```', '')
  }
  return lines.join('\n')
}

const js = jsPackages()
const rust = rustPackages()
const header = [
  '# Third-party notices',
  '',
  'Pointer redistributes the third-party software listed here inside its desktop,',
  'web, and server builds. Each entry names the package and the license it ships',
  'under; the license texts follow at the end of this file.',
  '',
  'Regenerate after any dependency change:',
  '',
  '```bash',
  'npm run licenses',
  '```',
  '',
  `Generated from ${js.length} JavaScript package(s) and ${rust.length} Rust crate(s).`,
  '',
].join('\n')

const body = [
  header,
  renderInventory('JavaScript packages', js),
  renderInventory('Rust crates', rust),
  renderLicenseTexts([...js, ...rust]),
].join('\n')

fs.writeFileSync(outFile, `${body.trimEnd()}\n`)
console.log(
  `[licenses] wrote ${path.relative(root, outFile)} — ${js.length} JS packages, ${rust.length} Rust crates`
)
