/**
 * UUID v4 for conversation / message / attachment client ids.
 *
 * Prefer `crypto.randomUUID()` when present. Some mobile browsers / embedded
 * WebViews expose `crypto` without `randomUUID` (or lack a secure context),
 * which previously crashed boot + send (`TypeError: crypto.randomUUID is not a function`).
 */
export function randomUuid(): string {
  const c = typeof globalThis !== 'undefined' ? globalThis.crypto : undefined
  if (c && typeof c.randomUUID === 'function') {
    return c.randomUUID()
  }
  if (c && typeof c.getRandomValues === 'function') {
    const bytes = new Uint8Array(16)
    c.getRandomValues(bytes)
    bytes[6] = (bytes[6] & 0x0f) | 0x40
    bytes[8] = (bytes[8] & 0x3f) | 0x80
    const hex = Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('')
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
  }
  // Last resort: still emit UUID-shaped opaque ids (not cryptographically strong).
  console.warn('[randomUuid] crypto unavailable; using Math.random fallback')
  const rnd = () => Math.floor(Math.random() * 0x10000).toString(16).padStart(4, '0')
  return `${rnd()}${rnd()}-${rnd()}-4${rnd().slice(1)}-${(8 + Math.floor(Math.random() * 4)).toString(16)}${rnd().slice(1)}-${rnd()}${rnd()}${rnd()}`
}
