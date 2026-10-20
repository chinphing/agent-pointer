/**
 * One JSON parse per `(tool call object, field, raw string)`.
 *
 * Why: one `ToolCallRow` render reads the same field of the same tool call from
 * several places — `arguments` from the header label, the path chip, the
 * summary and the `aria-label`; `result` from the terminal block, the
 * background-handle helpers (`resolveBackgroundHostDisplayStatus` alone reached
 * the parse four to six times) and the task-board line. Un-cached, one render
 * re-parsed the same string 5–8 times.
 *
 * Shape, and why:
 *
 * - **Keyed by the call object first** (`WeakMap`), so two calls that happen to
 *   carry the same string never share a parsed value — a caller that mutated
 *   what it got back cannot leak into another row. The entry dies with the
 *   object.
 * - **Invalidated by the raw string**: the stored `raw` is compared on every
 *   read, so a replaced field re-parses exactly once. `arguments` only ever
 *   grows while a call streams; `result` is written when the call completes and
 *   deleted by `slimToolCallBody`, so the comparison is a pointer/length check
 *   in the common case.
 * - **The parser owns the semantics**: `arguments` refuses to parse a payload
 *   that is still arriving (`parseCompleteToolCallArgumentsObject` → `null`),
 *   while `result` uses plain `JSON.parse` and reports `ok: false` instead of
 *   throwing — so every call site keeps its own fallback without wrapping each
 *   read in `try`/`catch`.
 *
 * A returned `value` is shared with the cache: treat it as **read-only**.
 */

/** Outcome of parsing one field. `ok: false` means "no JSON value here". */
export interface FieldParse<T> {
  ok: boolean
  value: T
}

/** A cached parse, plus the raw string it was produced from. */
export interface CachedField<T> extends FieldParse<T> {
  raw: string
}

type FieldParser<T> = (raw: string) => FieldParse<T>

let stores = new WeakMap<object, Map<string, CachedField<unknown>>>()

/**
 * Test-only: drop every cached entry. A `WeakMap` cannot be cleared in place,
 * so the store is replaced; the per-object maps go with it.
 */
export function resetToolCallFieldCache(): void {
  stores = new WeakMap()
}

/**
 * Parse `raw` for `owner`'s `field`, or reuse the previous parse of the very
 * same string. `parse` must be a stable function reference (module-level) so a
 * read never allocates a closure.
 */
export function readCachedField<T>(
  owner: object,
  field: string,
  raw: string,
  parse: FieldParser<T>
): CachedField<T> {
  const store = stores.get(owner)
  const hit = store?.get(field)
  if (hit && hit.raw === raw) return hit as CachedField<T>

  const parsed = parse(raw)
  const entry: CachedField<T> = { raw, ok: parsed.ok, value: parsed.value }
  if (store) store.set(field, entry as CachedField<unknown>)
  else stores.set(owner, new Map([[field, entry as CachedField<unknown>]]))
  return entry
}
