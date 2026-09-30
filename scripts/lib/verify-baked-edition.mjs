/**
 * Guardrail for `npm run server:build`: the two halves of a release must agree
 * on the packaging flavour.
 *
 * A pointer-server release is bound only when **both** halves carry the control
 * plane:
 *   • the Rust binary  ← `POINTER_EDITION=managed` + the three domains, baked by
 *                        `crates/pointer-core/build.rs` (`cargo:rustc-env=…`);
 *   • the web assets   ← `VITE_POINTER_EDITION` / `VITE_POINTER_WEB_BASE` inlined
 *                        by Vite.
 *
 * Loading `pointer.local.env` in only one of the two build steps used to produce
 * a silently half-bound artifact (managed UI over a standalone binary). This
 * module makes that state loud.
 *
 * Everything here is pure I/O: read the artifacts, report findings. No shell
 * (`strings` / `grep` are not available on every Windows box) and no exit — the
 * caller decides what to do with `ok === false`.
 */
import fs from 'node:fs'
import path from 'node:path'

/** Domains `crates/pointer-core/build.rs` bakes into the binary, in its order. */
export const CONTROL_PLANE_DOMAINS = [
  { key: 'POINTER_API_BASE', label: 'control-plane API base' },
  { key: 'POINTER_WEB_BASE', label: 'control-plane web base' },
  { key: 'COMPUTER_ANNOTATE_API_BASE', label: 'SOM annotate base' },
]

const CHUNK_BYTES = 64 * 1024
const MAX_SCAN_BYTES = 256 * 1024 * 1024

/**
 * Effective packaging flavour. `managed` only for the one accepted value; every
 * other state (unset, blank, anything else) means standalone — same rule as
 * `crates/pointer-core/src/edition.rs`.
 */
export function effectiveEdition(env = {}) {
  const raw = String(env.POINTER_EDITION ?? env.VITE_POINTER_EDITION ?? '')
    .trim()
    .toLowerCase()
  return raw === 'managed' ? 'managed' : 'standalone'
}

/** Configured control-plane domains, in `build.rs` order; blanks are skipped. */
export function configuredDomains(env = {}) {
  const found = []
  for (const { key, label } of CONTROL_PLANE_DOMAINS) {
    const value = String(env[key] ?? '').trim()
    if (value) found.push({ key, label, value })
  }
  return found
}

/** Web base the frontend inlines (`VITE_POINTER_WEB_BASE`, mirrored from POINTER_WEB_BASE). */
export function configuredWebBase(env = {}) {
  return String(env.POINTER_WEB_BASE ?? env.VITE_POINTER_WEB_BASE ?? '').trim()
}

/** Byte-exact substring search over one file, read in chunks. */
export function fileContainsBytes(filePath, needle) {
  const needleBuf = Buffer.from(String(needle), 'utf8')
  if (needleBuf.length === 0) return false

  const size = fs.statSync(filePath).size
  if (size === 0 || size > MAX_SCAN_BYTES) return false

  const fd = fs.openSync(filePath, 'r')
  try {
    const chunk = Buffer.allocUnsafe(CHUNK_BYTES)
    let carry = Buffer.alloc(0)
    let position = 0
    while (position < size) {
      const read = fs.readSync(fd, chunk, 0, CHUNK_BYTES, position)
      if (read <= 0) break
      position += read

      const window = carry.length > 0 ? Buffer.concat([carry, chunk.subarray(0, read)]) : chunk.subarray(0, read)
      if (window.indexOf(needleBuf) !== -1) return true

      // Keep the tail so a needle straddling a chunk boundary is still found.
      const keep = Math.min(needleBuf.length - 1, window.length)
      carry = keep > 0 ? Buffer.from(window.subarray(window.length - keep)) : Buffer.alloc(0)
    }
    return false
  } finally {
    fs.closeSync(fd)
  }
}

function listFilesRecursive(root) {
  const out = []
  for (const entry of fs.readdirSync(root, { recursive: true })) {
    const full = path.join(root, String(entry))
    try {
      if (fs.statSync(full).isFile()) out.push(full)
    } catch {
      // Unreadable entry — ignore; a missing hit still surfaces as a finding.
    }
  }
  return out
}

/**
 * Search a file, or every file under a directory.
 * @returns {boolean|null} `null` when the target does not exist.
 */
export function pathContains(targetPath, needle) {
  let stat
  try {
    stat = fs.statSync(targetPath)
  } catch {
    return null
  }
  if (stat.isDirectory()) {
    for (const file of listFilesRecursive(targetPath)) {
      try {
        if (fileContainsBytes(file, needle)) return true
      } catch {
        // Skip unreadable file; keep scanning the rest.
      }
    }
    return false
  }
  return fileContainsBytes(targetPath, needle)
}

/** @returns {(needle: string) => (boolean|null)} */
export function createPathProbe(targetPath) {
  return (needle) => pathContains(targetPath, needle)
}

function displayLabel(targetPath) {
  const relative = path.relative(process.cwd(), targetPath)
  return relative && !relative.startsWith('..') ? relative : targetPath
}

/**
 * Pure core: compare the effective flavour against what the artifacts carry.
 *
 * @param {object} opts
 * @param {'managed'|'standalone'} opts.edition
 * @param {{key:string,label:string,value:string}[]} [opts.domains]
 * @param {string} [opts.webBase]
 * @param {((needle: string) => (boolean|null))|null} [opts.binaryProbe]
 * @param {((needle: string) => (boolean|null))|null} [opts.webProbe]
 * @param {string} [opts.binaryLabel]
 * @param {string} [opts.webLabel]
 * @returns {{ok:boolean,errors:string[],warnings:string[],notes:string[],summary:string}}
 */
export function verifyBakedEdition({
  edition,
  domains = [],
  webBase = '',
  binaryProbe = null,
  webProbe = null,
  binaryLabel = 'release binary',
  webLabel = 'web assets',
}) {
  const managed = edition === 'managed'
  const errors = []
  const warnings = []
  const notes = []

  if (!binaryProbe) {
    if (managed) {
      errors.push(`${binaryLabel} not found — a managed build cannot be verified; run the full build first`)
    } else {
      notes.push(`${binaryLabel} not found (nothing baked to report)`)
    }
  }

  let baked = 0
  for (const domain of domains) {
    const hit = binaryProbe ? binaryProbe(domain.value) : null
    if (hit === true) {
      baked += 1
      continue
    }
    if (hit === false) {
      if (managed) {
        errors.push(`${domain.key} (${domain.label}) is NOT baked into ${binaryLabel}: ${domain.value}`)
      } else {
        notes.push(`${domain.key} not baked into ${binaryLabel}`)
      }
      continue
    }
    if (managed && binaryProbe) {
      errors.push(`${domain.key} could not be checked in ${binaryLabel}`)
    }
  }

  let webBaked = false
  if (webBase) {
    const hit = webProbe ? webProbe(webBase) : null
    webBaked = hit === true
    if (hit === true) {
      notes.push(`${webLabel} carry the web base`)
    } else if (hit === false) {
      if (managed) {
        errors.push(`${webLabel} do NOT carry POINTER_WEB_BASE: ${webBase}`)
      } else {
        warnings.push(`${webLabel} carry POINTER_WEB_BASE (${webBase}) although the edition is standalone`)
      }
    } else if (managed) {
      errors.push(`${webLabel} not found — cannot verify the managed web base (${webBase})`)
    } else {
      notes.push(`${webLabel} not found (nothing baked to report)`)
    }
  } else if (managed) {
    errors.push('no POINTER_WEB_BASE configured — a managed build must inject the control-plane domains')
  }

  if (!managed) {
    for (const domain of domains) {
      if (binaryProbe && binaryProbe(domain.value) === true) {
        warnings.push(
          `${domain.key} is baked into ${binaryLabel} (${domain.value}) although the edition is standalone`,
        )
      }
    }
  }

  const summary =
    `${managed ? 'edition=managed' : 'edition=standalone'} ` +
    `domains=${baked}/${domains.length} baked, web=${webBaked ? 'managed' : 'standalone'}`

  return { ok: errors.length === 0, errors, warnings, notes, summary }
}

/**
 * Read the real artifacts at `binaryPath` / `webDir` and run the core check
 * against `env` (already merged with `pointer.local.env`).
 */
export function verifyBuildArtifacts({ env = {}, binaryPath, webDir }) {
  return verifyBakedEdition({
    edition: effectiveEdition(env),
    domains: configuredDomains(env),
    webBase: configuredWebBase(env),
    binaryProbe: fs.existsSync(binaryPath) ? createPathProbe(binaryPath) : null,
    webProbe: fs.existsSync(webDir) ? createPathProbe(webDir) : null,
    binaryLabel: displayLabel(binaryPath),
    webLabel: `${displayLabel(webDir)}/`,
  })
}
