/**
 * Parsed `toolCall.result`, cached per `(tool call object, result string)`.
 *
 * The `result` side of the same problem `lib/toolCallArguments.ts` solves: one
 * render asked for the same result JSON from the terminal block, the
 * background-handle helpers, the task-board line and the web-search sources, so
 * the same string was parsed four to six times (`resolveBackgroundHostDisplayStatus`
 * alone parsed it five times through `isBackgroundJobHandleResult` /
 * `backgroundHandleStatus`).
 *
 * Differences from `arguments`, which make this side simpler:
 *
 * - A `result` is always a **complete** JSON document — it arrives when the call
 *   finishes, so there is no half-written payload to pre-check and no streaming
 *   branch. Only the plain `JSON.parse` outcome matters.
 * - Nothing here throws: `ok: false` replaces the per-call `try`/`catch`, and
 *   each call site keeps its own fallback (`null`, `{}`, the raw string).
 *
 * The returned value is shared with the cache — **read only**. Anything that
 * needs to mutate a result (e.g. `rewriteHandleStatus` in the store helpers)
 * must keep parsing its own private copy; that is why a bare string source is
 * parsed without caching.
 */

import type { ToolCall } from '../types/chat'
import { readCachedField, type CachedField, type FieldParse } from './toolCallFieldCache'

/** A tool call, or the bare `result` string when that is all a caller holds. */
export type ToolCallResultSource = Pick<ToolCall, 'result'> | string | null | undefined

function sourceRaw(source: ToolCallResultSource): string {
  if (typeof source === 'string') return source
  if (!source || typeof source !== 'object') return ''
  return typeof source.result === 'string' ? source.result : ''
}

function parseResult(raw: string): FieldParse<unknown> {
  // Empty / whitespace-only is "no result yet" — the same thing the call sites'
  // `!result?.trim()` guards already said, without a guaranteed throw.
  if (!raw.trim()) return { ok: false, value: null }
  try {
    return { ok: true, value: JSON.parse(raw) }
  } catch {
    return { ok: false, value: null }
  }
}

/** The raw result string a source carries (`''` when there is none). */
export function toolCallResultRaw(source: ToolCallResultSource): string {
  return sourceRaw(source)
}

/**
 * `ok` + parsed JSON value for a result, at most once per `(call, string)`.
 *
 * A bare string is parsed without caching: it has no object to key on, and the
 * one string-only caller (`rewriteHandleStatus`) mutates what it parses.
 */
export function toolCallResultJson(source: ToolCallResultSource): CachedField<unknown> {
  const raw = sourceRaw(source)
  if (typeof source !== 'object' || source === null) {
    return { raw, ...parseResult(raw) }
  }
  return readCachedField(source, 'result', raw, parseResult)
}

/**
 * The result as a JSON object (arrays included, since a property read on one is
 * as harmless as it was on the `JSON.parse` result), or `null` when the result
 * is absent, unparseable, or a scalar.
 */
export function toolCallResultObject(
  source: ToolCallResultSource
): Record<string, unknown> | null {
  const parsed = toolCallResultJson(source)
  if (!parsed.ok) return null
  const value = parsed.value
  if (!value || typeof value !== 'object') return null
  return value as Record<string, unknown>
}
