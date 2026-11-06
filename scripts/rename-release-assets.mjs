/**
 * Copy freshly built installers into one folder and mark the flavour in the
 * file name. Tauri and the server packager keep their own names; the release
 * workflow calls this once per flavour so official and community artifacts
 * can sit on the same GitHub Release without overwriting each other.
 *
 *   node scripts/rename-release-assets.mjs --flavor community --dest release-assets
 *   node scripts/rename-release-assets.mjs --flavor official --dest release-assets
 */
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const FLAVORS = new Set(['community', 'official'])

const SEARCH_ROOTS = [
  'target/release/bundle',
  'target/universal-apple-darwin/release/bundle',
  'target/release/pointer-server-bundle',
]

export function releaseAssetName(filename, flavor) {
  if (!FLAVORS.has(flavor)) {
    throw new Error(`release flavor must be community or official, got ${flavor}`)
  }
  if (filename.startsWith(`Pointer_${flavor}_`) || filename.startsWith(`pointer-server-${flavor}`)) {
    return filename
  }
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
