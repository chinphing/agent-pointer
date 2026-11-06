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
  COMPUTER_ANNOTATE_API_BASE: 'https://pointer-som.readflowai.com',
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
    ? `pointer-server-${label}_${version}_${arch}`
    : `pointer-server_${version}_${arch}`
}

const SEARCH_ROOTS = [
  'target/release/bundle',
  'target/universal-apple-darwin/release/bundle',
  'target/release/pointer-server-bundle',
]

function alreadyMarked(filename) {
  return (
    /^Pointer_(community|official)_/.test(filename)
    || /^pointer-server-(community|official)([-_]|$)/.test(filename)
  )
}

export function releaseAssetName(filename, flavor) {
  if (!FLAVORS.has(flavor)) {
    throw new Error(`release flavor must be community or official, got ${flavor}`)
  }
  if (alreadyMarked(filename)) return filename
  if (filename.startsWith('Pointer_')) {
    return `Pointer_${flavor}_${filename.slice('Pointer_'.length)}`
  }
  if (filename.startsWith('pointer-server-')) {
    return `pointer-server-${flavor}-${filename.slice('pointer-server-'.length)}`
  }
  if (filename.startsWith('pointer-server_')) {
    return `pointer-server-${flavor}_${filename.slice('pointer-server_'.length)}`
  }
  throw new Error(`cannot mark release file ${filename}`)
}

function isReleaseFile(filename) {
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
