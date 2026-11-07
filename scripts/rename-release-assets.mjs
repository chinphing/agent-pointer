/**
 * Mark community and official installers.
 * `npm run tauri:build` renames Tauri's bundles in place.
 * The server packager writes the marker into the zip and deb names.
 * The release workflow then copies those already marked files onto one Release.
 *
 *   node scripts/rename-release-assets.mjs --flavor community --dest release-assets
 *   node scripts/rename-release-assets.mjs --flavor official --dest release-assets
 */
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const FLAVORS = new Set(['community', 'official'])

const OFFICIAL_HOSTS = {
  POINTER_API_BASE: 'https://pointer-api.readflowai.com',
  POINTER_WEB_BASE: 'https://pointer.readflowai.com',
}

function normalizedHost(value) {
  return String(value ?? '').trim().replace(/\/+$/, '').toLowerCase()
}

/** community, official, or null when this build is an enterprise package. */
export function resolveReleaseLabel(env) {
  const explicit = String(env.POINTER_PACKAGE_LABEL ?? '').trim().toLowerCase()
  if (explicit === 'community' || explicit === 'official') return explicit
  if (explicit) {
    throw new Error(`POINTER_PACKAGE_LABEL must be community or official, got ${explicit}`)
  }
  const edition = String(env.POINTER_EDITION ?? '').trim().toLowerCase()
  if (!edition) return 'community'
  if (edition !== 'managed') return null
  const official = Object.entries(OFFICIAL_HOSTS).every(
    ([key, expected]) => normalizedHost(env[key]) === expected,
  )
  return official ? 'official' : null
}

export function serverZipBaseName(platform, env) {
  const label = resolveReleaseLabel(env)
  return label ? `pointer-server-${label}-${platform}` : `pointer-server-${platform}`
}

export function serverDebBaseName(version, arch, env) {
  const label = resolveReleaseLabel(env)
  return label
    ? `pointer-server-${label}-${version}-${arch}`
    : `pointer-server_${version}_${arch}`
}

const SEARCH_ROOTS = [
  'target/release/bundle',
  'target/universal-apple-darwin/release/bundle',
  'target/release/pointer-server-bundle',
]

const RELEASE_EXTS = [
  '.deb.sig',
  '.app.tar.gz',
  '.nsis.zip',
  '.tar.gz',
  '.AppImage',
  '.deb',
  '.msi',
  '.exe',
  '.dmg',
  '.zip',
  '.sig',
]

function splitReleaseExt(filename) {
  const lower = filename.toLowerCase()
  for (const ext of RELEASE_EXTS) {
    if (lower.endsWith(ext.toLowerCase())) {
      return [filename.slice(0, -ext.length), filename.slice(-ext.length)]
    }
  }
  return [filename, '']
}

function withFlavor(prefix, rest, flavor) {
  const body = rest.replace(/^(community|official)[-_]/, '').replaceAll('_', '-')
  return `${prefix}-${flavor}-${body}`
}

export function releaseAssetName(filename, flavor) {
  return stripInstallerLocale(markReleaseName(filename, flavor))
}

/**
 * Tauri appends the WiX culture to MSI names (`Pointer_0.1.2_x64_zh-CN.msi`). The
 * app ships every locale in one package, so that tag names the installer wizard's
 * language, not the product — publishing it suggests a language-specific build.
 * Strip it from the published name; the MSI's embedded language is untouched.
 * `…_zh-CN.msi.sig` carries the installer's name, so it is stripped the same way.
 */
function stripInstallerLocale(filename) {
  const [stem, ext] = splitReleaseExt(filename)
  const lowerExt = ext.toLowerCase()
  if (lowerExt === '.msi') return `${stripMsiCulture(stem)}${ext}`
  if (lowerExt === '.sig' && stem.toLowerCase().endsWith('.msi')) {
    return `${stripMsiCulture(stem.slice(0, -'.msi'.length))}.msi${ext}`
  }
  return filename
}

/** `…-x64-zh-CN` → `…-x64`; an architecture tag (`x64`, `aarch64`) never matches. */
function stripMsiCulture(stem) {
  return stem.replace(/[_-][a-z]{2}(?:-[A-Z]{2})?$/, '')
}

function markReleaseName(filename, flavor) {
  if (!FLAVORS.has(flavor)) {
    throw new Error(`release flavor must be community or official, got ${flavor}`)
  }
  const [stem, ext] = splitReleaseExt(filename)
  const marked = `Pointer-${flavor}`
  const alreadyMarked =
    stem === marked ||
    stem.startsWith(`${marked}-`) ||
    stem.startsWith(`${marked}.`) ||
    stem.startsWith(`pointer-server-${flavor}-`)
  if (alreadyMarked) {
    return filename
  }
  // Tauri's updater tarball carries no separator: `Pointer.app.tar.gz` and its `.sig`.
  if (stem === 'Pointer' || stem.startsWith('Pointer.')) {
    return `${marked}${stem.slice('Pointer'.length)}${ext}`
  }
  if (stem.startsWith('pointer-server-') || stem.startsWith('pointer-server_')) {
    return `${withFlavor('pointer-server', stem.slice('pointer-server-'.length), flavor)}${ext}`
  }
  if (stem.startsWith('Pointer-') || stem.startsWith('Pointer_')) {
    return `${withFlavor('Pointer', stem.slice('Pointer_'.length), flavor)}${ext}`
  }
  throw new Error(`cannot mark release file ${filename}`)
}

function isReleaseFile(filename) {
  // Bundle trees also carry loose assets such as the AppImage icon (`Pointer.png`)
  // or the `.Dir` payload; only files with a known release extension are installers.
  const lower = filename.toLowerCase()
  if (!RELEASE_EXTS.some((ext) => lower.endsWith(ext.toLowerCase()))) return false
  return (
    filename.startsWith('Pointer')
    || filename.startsWith('pointer-server-')
    || filename.startsWith('pointer-server_')
  )
}

function walkFiles(dir, out) {
  if (!fs.existsSync(dir)) return
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      walkFiles(full, out)
      continue
    }
    if (entry.isFile() && isReleaseFile(entry.name)) out.push(full)
  }
}

export function collectReleaseFiles(root) {
  const found = []
  for (const rel of SEARCH_ROOTS) walkFiles(path.join(root, rel), found)
  return found.sort()
}

export function markBuiltReleaseFiles(root, env) {
  const flavor = resolveReleaseLabel(env)
  if (!flavor) return []
  const renamed = []
  for (const file of collectReleaseFiles(root)) {
    const next = releaseAssetName(path.basename(file), flavor)
    if (next === path.basename(file)) continue
    const dest = path.join(path.dirname(file), next)
    fs.renameSync(file, dest)
    renamed.push(dest)
  }
  return renamed
}

export function stageReleaseAssets({ root, flavor, dest }) {
  const files = collectReleaseFiles(root)
  if (files.length === 0) {
    throw new Error(`no release artifacts to mark as ${flavor} under ${root}`)
  }
  fs.mkdirSync(dest, { recursive: true })
  const copied = []
  for (const file of files) {
    const name = releaseAssetName(path.basename(file), flavor)
    const target = path.join(dest, name)
    fs.copyFileSync(file, target)
    copied.push(target)
  }
  return copied
}

function argValue(argv, flag) {
  const index = argv.indexOf(flag)
  if (index === -1 || index + 1 >= argv.length) {
    throw new Error(`missing ${flag}`)
  }
  return argv[index + 1]
}

function isDirectRun() {
  const entry = process.argv[1]
  if (!entry) return false
  return path.resolve(entry) === fileURLToPath(import.meta.url)
}

if (isDirectRun()) {
  const flavor = argValue(process.argv.slice(2), '--flavor')
  const dest = path.resolve(argValue(process.argv.slice(2), '--dest'))
  const copied = stageReleaseAssets({ root: process.cwd(), flavor, dest })
  for (const file of copied) {
    console.log(`[release-assets] ${path.basename(file)}`)
  }
}
