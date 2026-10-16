/**
 * Dev-only `JSON.parse` meter, surfaced by the render-perf HUD.
 *
 * Why it exists: a real-device `sample` of the WebContent process caught a
 * ~966-sample JS call whose stack held two `JSC::jsonProtoFuncParse` frames
 * (~331 ms of parsing inside one call). `sample` cannot name JIT-ed JS, so the
 * only way to point at the call site is to measure the parse itself and keep the
 * stack of the slow ones — a `file:line` per slow call is what the HUD prints.
 *
 * Shape, and why:
 *
 * - **Nothing here patches anything by itself.** `createJsonParseMeter` /
 *   `recordJsonParseSample` / `formatStackFrames` / `formatJsonParseLines` are
 *   pure bookkeeping with an injectable clock, so the accounting is unit-tested
 *   instead of hidden in a closure around `JSON.parse`.
 * - **`installJsonParseProbe` is the only side effect**: it swaps `JSON.parse`
 *   for a wrapper and returns the disposer that puts the original back. The
 *   caller owns the lifecycle (`lib/renderPerf.ts` installs it on HUD enable and
 *   disposes it on disable), so while the HUD is off `JSON.parse` is the
 *   untouched native function — zero cost, zero behavior change.
 * - **The wrapper forwards everything.** Same arguments (reviver included), same
 *   return value, same thrown error, and it never coerces the input: the length
 *   is only read when the input already *is* a string, so a call that used to
 *   throw a `SyntaxError` still throws exactly that.
 * - **Stacks are captured only above the threshold** (default 5 ms). A capture
 *   costs a real `Error` construction, so it must never ride the hot path; the
 *   slow samples are kept slowest-first and capped (`keep`, default 3) because
 *   the HUD has room for a handful of lines, not a session.
 * - **Frames are `file:line`, innermost first.** A raw V8/JSC stack line carries
 *   the origin and a cache-busting query string; both are noise in a HUD that is
 *   meant to be read at a glance, so a location is shortened to its last three
 *   path segments (`src/lib/subAgentMessages.ts:459`) and the probe's own frame
 *   is dropped.
 */

/** A parse **above** this duration gets its stack captured, in ms. */
export const JSON_PARSE_SLOW_MS = 5
/** How many slow samples the meter keeps (the HUD prints these). */
export const JSON_PARSE_KEEP = 3
/** Frames kept per captured stack — enough to name the caller, not a whole trace. */
export const JSON_PARSE_STACK_FRAMES = 3
/**
 * Frames whose location contains one of these are dropped: they are the meter's
 * own wrapper, never the interesting call site.
 */
export const JSON_PARSE_STACK_SKIP = ['jsonParseProbe'] as const

/** One parse slow enough to keep, with the caller's location. */
export interface JsonParseSlowSample {
  /** Duration of the call, in ms. */
  ms: number
  /** `String(input).length` — the input's character count. */
  chars: number
  /** `file:line` frames, innermost first (see `formatStackFrames`). */
  frames: string[]
}

/** Everything the meter accumulates. Mutated in place by the recorder. */
export interface JsonParseStats {
  /** Total calls measured. */
  calls: number
  /** Summed duration of every measured call, in ms. */
  totalMs: number
  /** Summed input characters of every measured call. */
  totalChars: number
  /** Calls that threw. Counted, never stack-captured. */
  failed: number
  /** Duration of the slowest single call, in ms. */
  maxMs: number
  /** Input characters of that slowest call. */
  maxChars: number
  /** Caller frames of that slowest call. */
  maxFrames: string[]
  /** Slow samples, slowest first, capped at `keep`. */
  slow: JsonParseSlowSample[]
  /** Slow calls seen in total — may exceed `slow.length` once the cap bites. */
  slowTotal: number
}

export interface JsonParseMeterOptions {
  /** Capture threshold in ms; defaults to `JSON_PARSE_SLOW_MS`. */
  slowMs?: number
  /** Kept samples; defaults to `JSON_PARSE_KEEP`. */
  keep?: number
  /** Clock, in ms; defaults to `performance.now()` when available. */
  now?: () => number
  /** Stack source, defaults to `new Error().stack`. */
  captureStack?: () => string | undefined
}

/** Empty accumulator. */
export function createJsonParseStats(): JsonParseStats {
  return {
    calls: 0,
    totalMs: 0,
    totalChars: 0,
    failed: 0,
    maxMs: 0,
    maxChars: 0,
    maxFrames: [],
    slow: [],
    slowTotal: 0
  }
}

/**
 * Clear an accumulator **in place**. In place, not by replacement: the HUD holds
 * the very object `renderPerfSnapshot()` hands it, and swapping the reference
 * would leave the overlay reading a detached one.
 */
export function resetJsonParseStats(stats: JsonParseStats): void {
  stats.calls = 0
  stats.totalMs = 0
  stats.totalChars = 0
  stats.failed = 0
  stats.maxMs = 0
  stats.maxChars = 0
  stats.maxFrames = []
  stats.slow = []
  stats.slowTotal = 0
}

/** Input character count, read only when the input already is a string. */
export function jsonParseInputChars(input: unknown): number {
  return typeof input === 'string' ? input.length : 0
}

function defaultNow(): number {
  if (typeof performance !== 'undefined' && typeof performance.now === 'function') {
    return performance.now()
  }
  return Date.now()
}

/**
 * `src/lib/subAgentMessages.ts` out of
 * `http://localhost:5173/src/lib/subAgentMessages.ts?t=1234` (and the same for a
 * `file://` URL, a bare path, or a webpack-style bundle name): drop the origin
 * and the query/fragment, then keep the last three segments so the file name and
 * its folder survive in a few columns.
 */
export function shortenStackLocation(location: string): string {
  let value = location.split('?')[0] ?? ''
  value = value.split('#')[0] ?? ''
  const scheme = /^[a-z][a-z0-9+.-]*:\/\//i.exec(value)
  if (scheme) {
    value = value.slice(scheme[0].length)
    const slash = value.indexOf('/')
    value = slash >= 0 ? value.slice(slash) : value
  }
  const parts = value.split('/').filter(part => part.length > 0)
  return parts.length <= 3 ? parts.join('/') : parts.slice(-3).join('/')
}

/** `Error` header, `at` prefix, `@` prefix and `(…)` wrapper all tolerated. */
const STACK_LOCATION_RE = /(?:^|[@(])([^()\s@]+?):(\d+):(\d+)\)?$/

/**
 * `file:line` out of one raw stack line, or `''` when the line carries no
 * location (a `<anonymous>` frame, the `Error` header, a native frame).
 *
 * Both frame dialects are handled: Chrome's `at fn (url:line:col)` / `at
 * url:line:col` and Firefox's `fn@url:line:col`.
 */
export function stackFrameLocation(frameLine: string): string {
  const line = frameLine.trim().replace(/^at\s+/, '')
  const match = STACK_LOCATION_RE.exec(line)
  if (!match) return ''
  const file = shortenStackLocation(match[1] ?? '')
  if (!file) return ''
  return `${file}:${match[2]}`
}

export interface FormatStackOptions {
  /** Frames kept; defaults to `JSON_PARSE_STACK_FRAMES`. */
  maxFrames?: number
  /** Locations containing one of these substrings are dropped. */
  skip?: readonly string[]
}

/**
 * Turn a raw `Error.stack` into `file:line` frames, innermost first: the first
 * entry is where the parse happened, the next ones are its callers. The meter's
 * own wrapper frame is skipped, duplicates are collapsed and the list is capped —
 * the HUD prints this on one line.
 */
export function formatStackFrames(
  stack: string | undefined,
  options: FormatStackOptions = {}
): string[] {
  if (!stack) return []
  const maxFrames = options.maxFrames ?? JSON_PARSE_STACK_FRAMES
  if (maxFrames <= 0) return []
  const skip = options.skip ?? JSON_PARSE_STACK_SKIP
  const frames: string[] = []
  for (const line of stack.split('\n')) {
    if (frames.length >= maxFrames) break
    const location = stackFrameLocation(line)
    if (!location) continue
    if (skip.some(needle => location.includes(needle))) continue
    if (frames[frames.length - 1] === location) continue
    frames.push(location)
  }
  return frames
}

export interface JsonParseSample {
  ms: number
  chars: number
  /** Caller frames; captured by the caller, so the recorder stays pure. */
  frames?: readonly string[]
  /** The call threw — counted, never kept as a slow sample. */
  failed?: boolean
}

/**
 * Fold one measured call into `stats`. Mutates in place (the meter runs once per
 * `JSON.parse` call and must not allocate), and never throws on odd input: a
 * non-finite duration is ignored, a negative one is clamped to zero.
 */
export function recordJsonParseSample(
  stats: JsonParseStats,
  sample: JsonParseSample,
  options: JsonParseMeterOptions = {}
): void {
  const slowMs = options.slowMs ?? JSON_PARSE_SLOW_MS
  const keep = options.keep ?? JSON_PARSE_KEEP
  const ms = Number.isFinite(sample.ms) ? Math.max(sample.ms, 0) : 0
  const chars = Number.isFinite(sample.chars) ? Math.max(sample.chars, 0) : 0

  stats.calls += 1
  stats.totalMs += ms
  stats.totalChars += chars
  if (sample.failed) stats.failed += 1

  if (ms > stats.maxMs) {
    stats.maxMs = ms
    stats.maxChars = chars
    stats.maxFrames = [...(sample.frames ?? [])]
  }

  if (ms <= slowMs || keep <= 0) return
  stats.slowTotal += 1
  const entry: JsonParseSlowSample = { ms, chars, frames: [...(sample.frames ?? [])] }
  const at = stats.slow.findIndex(kept => kept.ms < entry.ms)
  if (at < 0) stats.slow.push(entry)
  else stats.slow.splice(at, 0, entry)
  if (stats.slow.length > keep) stats.slow.length = keep
}

export interface JsonParseMeter extends JsonParseMeterOptions {
  stats: JsonParseStats
  /** Measure `parse` and fold the sample in. Returns whatever `parse` returned. */
  measure: <T>(parse: () => T) => T
}

/**
 * Bookkeeping without touching the global: the caller owns `stats` (the HUD reads
 * the same object) and the clock, which is what makes the meter testable.
 */
export function createJsonParseMeter(options: JsonParseMeterOptions = {}): JsonParseMeter {
  const stats = createJsonParseStats()
  const now = options.now ?? defaultNow
  const captureStack = options.captureStack ?? (() => new Error().stack)
  const slowMs = options.slowMs ?? JSON_PARSE_SLOW_MS
  const keep = options.keep ?? JSON_PARSE_KEEP

  return {
    stats,
    slowMs,
    keep,
    now,
    captureStack,
    measure<T>(parse: () => T): T {
      const startedAt = now()
      let result: T
      try {
        result = parse()
      } catch (error) {
        recordJsonParseSample(
          stats,
          { ms: now() - startedAt, chars: 0, failed: true },
          { slowMs, keep }
        )
        throw error
      }
      const ms = now() - startedAt
      recordJsonParseSample(
        stats,
        { ms, chars: 0, frames: ms > slowMs ? formatStackFrames(captureStack()) : undefined },
        { slowMs, keep }
      )
      return result
    }
  }
}

/** Just enough of `JSON` to be swapped in and out. */
export interface JsonParseTarget {
  parse: (text: string, reviver?: (this: unknown, key: string, value: unknown) => unknown) => unknown
}

export interface JsonParseProbeOptions extends JsonParseMeterOptions {
  /** Accumulator to fill; defaults to a fresh one (read it off the return value). */
  stats?: JsonParseStats
  /** What to wrap; defaults to the global `JSON`. */
  target?: JsonParseTarget
  /** Stack frames kept per slow call; defaults to `JSON_PARSE_STACK_FRAMES`. */
  maxFrames?: number
}

/** Marker put on the wrapper, so a second install cannot stack a third layer. */
const PROBE_MARKER = '__pointerJsonParseProbe'

type MarkedParse = JsonParseTarget['parse'] & { [PROBE_MARKER]?: boolean }

/**
 * Swap `target.parse` (the global `JSON` by default) for a measuring wrapper and
 * return the disposer that restores the original.
 *
 * The wrapper is behavior-preserving by construction: it forwards both arguments
 * (`reviver` included), returns the original's value, rethrows the original's
 * error, and reads the input length only when the input is already a string. It
 * is installed *only* while the HUD is on — the disposer is what makes the off
 * state the untouched native function rather than a passthrough layer.
 *
 * Installing twice is a no-op (the wrapper carries a marker), so a double enable
 * cannot nest wrappers and multiply the accounting.
 */
export function installJsonParseProbe(options: JsonParseProbeOptions = {}): () => void {
  const target: JsonParseTarget =
    options.target ?? (JSON as unknown as JsonParseTarget)
  const original = target.parse as MarkedParse
  if (original[PROBE_MARKER] === true) return () => {}

  const stats = options.stats ?? createJsonParseStats()
  const now = options.now ?? defaultNow
  const slowMs = options.slowMs ?? JSON_PARSE_SLOW_MS
  const keep = options.keep ?? JSON_PARSE_KEEP
  const maxFrames = options.maxFrames ?? JSON_PARSE_STACK_FRAMES
  const captureStack = options.captureStack ?? (() => new Error().stack)

  const patched: MarkedParse = function parse(text: string, reviver?: unknown): unknown {
    const startedAt = now()
    let result: unknown
    try {
      result = original.call(target, text, reviver as never)
    } catch (error) {
      recordJsonParseSample(
        stats,
        { ms: now() - startedAt, chars: jsonParseInputChars(text), failed: true },
        { slowMs, keep }
      )
      throw error
    }
    const ms = now() - startedAt
    recordJsonParseSample(
      stats,
      {
        ms,
        chars: jsonParseInputChars(text),
        frames:
          ms > slowMs
            ? formatStackFrames(captureStack(), { maxFrames })
            : undefined
      },
      { slowMs, keep }
    )
    return result
  }
  patched[PROBE_MARKER] = true

  target.parse = patched as JsonParseTarget['parse']

  return () => {
    // Only restore what we installed: if something else replaced it in the
    // meantime, that wrapper is now the live one and must not be dropped.
    if (target.parse === (patched as JsonParseTarget['parse'])) {
      target.parse = original
    }
  }
}

/** `1.2k` / `3.4M` — a character count in a few columns. */
export function formatParseChars(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0'
  if (value < 10_000) return String(Math.round(value))
  if (value < 1_000_000) return `${(value / 1000).toFixed(1)}k`
  return `${(value / 1_000_000).toFixed(2)}M`
}

/** `12.3ms` / `0.40ms` — durations read as ms everywhere in the HUD. */
export function formatParseMs(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0.0ms'
  return `${value.toFixed(1)}ms`
}

/**
 * HUD rows for the meter. Two shapes:
 *
 * - one summary row — calls, summed and maximum duration, summed input
 *   characters and how many calls crossed the threshold;
 * - one row per kept slow sample, slowest first, carrying the `file:line` chain
 *   that answers "which JS path parsed this".
 *
 * The summary row is always present (a stable block is easier to compare across
 * screenshots); the sample rows only exist once something was slow, and the
 * empty case says which of the two reasons applies.
 */
export function formatJsonParseLines(stats: JsonParseStats, maxRows = JSON_PARSE_KEEP): string[] {
  const lines = [
    `parse calls ${stats.calls}  total ${formatParseMs(stats.totalMs)}`
      + `  max ${formatParseMs(stats.maxMs)}  chars ${formatParseChars(stats.totalChars)}`
      + `  slow ${stats.slowTotal}`
      + (stats.failed > 0 ? `  failed ${stats.failed}` : '')
  ]
  const rows = Math.max(0, Math.min(maxRows, stats.slow.length))
  if (rows === 0) {
    lines.push(
      stats.calls === 0
        ? 'parse  (no JSON.parse call since the HUD opened)'
        : `parse  (no call above ${formatParseMs(JSON_PARSE_SLOW_MS)})`
    )
    return lines
  }
  for (let i = 0; i < rows; i += 1) {
    const sample = stats.slow[i]
    if (!sample) continue
    const where = sample.frames.length > 0 ? `  ${sample.frames.join(' < ')}` : ''
    lines.push(
      `parse #${i + 1} ${formatParseMs(sample.ms)} ${formatParseChars(sample.chars)} ch${where}`
    )
  }
  return lines
}
