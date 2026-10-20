/**
 * One parse per `(tool call object, arguments string)` pair.
 *
 * Why: a single `ToolCallRow` render asks for the same `arguments` string from
 * several places (header label, header summary, path chip, `aria-label`, the
 * collapsed-group line), and `chat.ts` appends to `tc.arguments` every
 * `TOOL_ARGS_DELTA_BATCH_MS` while a call streams. Un-cached, one render of one
 * row parsed the same growing string 5–8 times, so a long `file_write` turned
 * into an O(n²) parse storm (measured: 3,968 calls / 4.26M chars / 876 throws
 * in 66 s, all of it frontend).
 *
 * Two properties make this safe:
 *
 * - **Keyed by the call object**, so two calls that happen to share a string do
 *   not share a result, and the entry dies with the object (`WeakMap`).
 * - **Invalidated by the string itself**: `arguments` only ever grows during a
 *   stream, so the stored `raw` comparison is a length mismatch — O(1) — until
 *   the payload settles. A changed string always re-parses.
 *
 * Incomplete payloads are never parsed at all: {@link
 * parseCompleteToolCallArgumentsObject} pre-checks with
 * `endOfFirstJsonValue` and returns `null`, which is what removes the throws
 * (and the `failed` counter) from the streaming window.
 *
 * The cache itself is the shared `(call object, field, raw string)` store in
 * `lib/toolCallFieldCache.ts` — the same primitive backs `result` reads in
 * `lib/toolCallResult.ts`, so the invalidation rules only exist once.
 */

import type { ToolCall } from '../types/chat'
import { parseCompleteToolCallArgumentsObject } from './parseToolCallArguments'
import { readCachedField, resetToolCallFieldCache, type FieldParse } from './toolCallFieldCache'

/** Stable parser reference — `readCachedField` must not allocate a closure per read. */
function parseArguments(raw: string): FieldParse<Record<string, unknown> | null> {
  const value = parseCompleteToolCallArgumentsObject(raw)
  return { ok: value !== null, value }
}

/**
 * Test-only: drop every cached entry — `arguments` here and `result` in
 * `lib/toolCallResult.ts`, since both live in one store.
 */
export function resetToolCallArgumentsCache(): void {
  resetToolCallFieldCache()
}

/**
 * Parsed `arguments` for display / behavior checks, or `null` when the payload
 * is empty, still streaming, or not an object.
 *
 * Never throws: callers may keep using `null` as "no args yet" — which is also
 * exactly what the previous `try { JSON.parse } catch { {} }` fallbacks meant.
 */
export function toolCallArguments(
  tc: Pick<ToolCall, 'arguments'> | null | undefined
): Record<string, unknown> | null {
  if (!tc || typeof tc !== 'object') return null
  const raw = typeof tc.arguments === 'string' ? tc.arguments : ''
  return readCachedField(tc, 'arguments', raw, parseArguments).value
}
