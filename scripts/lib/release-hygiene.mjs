/**
 * Release-artifact hygiene: nothing per-developer or signing-related may ship.
 *
 * The packaging scripts copy explicit inputs (release binary, `dist/`, `skills/`,
 * the config example) and never the repository root, so this is defence in depth —
 * but a future edit could widen that list, and a hand-rolled `zip -r` of the worktree
 * definitely would. `.gitignore` keeps these files out of git; this keeps them out
 * of releases.
 */

/** @type {{ pattern: RegExp, why: string }[]} */
export const FORBIDDEN_RELEASE_PATHS = [
  { pattern: /(^|\/)pointer\.local\.env$/, why: 'per-developer control-plane config' },
  { pattern: /(^|\/)signing\.env$/, why: 'Apple signing credentials' },
  { pattern: /\.(p12|p8|pfx|key)$/i, why: 'signing certificate or key' },
  { pattern: /(^|\/)logs?(\/|$)/i, why: 'local runtime logs' },
  { pattern: /\.log$/i, why: 'local runtime log' },
]

/**
 * @param {Iterable<string>} entries File names — staging-tree paths or archive members.
 * @returns {{ entry: string, why: string }[]} Every entry that must not ship.
 */
export function findForbiddenReleasePaths(entries) {
  const hits = []
  for (const raw of entries) {
    const entry = String(raw).replace(/\\/g, '/')
    for (const { pattern, why } of FORBIDDEN_RELEASE_PATHS) {
      if (pattern.test(entry)) hits.push({ entry, why })
    }
  }
  return hits
}

/**
 * Abort packaging when the staged tree (or the finished archive) carries a file that
 * must never reach a release.
 *
 * @param {Iterable<string>} entries
 * @param {string} context Human-readable stage name for the error message.
 */
export function assertNoForbiddenReleasePaths(entries, context) {
  const hits = findForbiddenReleasePaths(entries)
  if (hits.length === 0) return
  console.error(`[release-hygiene] ${context} must not contain:`)
  for (const { entry, why } of hits) console.error(`  • ${entry} — ${why}`)
  console.error('[release-hygiene] Refusing to package. Fix the input list, not this check.')
  process.exit(1)
}
