import { getCurrentScope, onScopeDispose, ref, type Ref } from 'vue'
import type { MemoryStats } from './memoryProbe'

/**
 * Dev-only render performance instrumentation, surfaced by
 * `components/dev/RenderPerfHud.vue`.
 *
 * Why it is shaped like this:
 *
 * - **Off by default, free when off.** `renderPerfEnabled()` is a plain boolean
 *   read. Every instrumentation point early-returns on it, the HUD component is
 *   not mounted, the rAF probe is not scheduled and the snapshot interval does
 *   not exist. The only costs while off are the (single) keydown listener and one
 *   sampler reference registered by a mounted `MessageList` — never a read.
 * - **No allocation on the hot path.** `bump` / `record` / `setGauge` mutate
 *   existing `Map` entries. Records are rebuilt once per snapshot (4 Hz), never
 *   per render.
 * - **Do not wrap hot call sites in a callback helper.** A `measurePerf(name, fn)`
 *   style helper allocates a closure per call even while disabled; call sites
 *   instead read `renderPerfEnabled()` once and branch.
 * - **Two clocks of evidence.** Frame stats are a live trailing 1 s window
 *   (fps + longest gap); render/mount counters are reported for the last
 *   *completed* second so the numbers do not jitter between snapshots.
 * - **Peaks are free, and they are what survives a screenshot.** A per-second
 *   counter is zero by the time a HUD screenshot is taken after a scroll, so
 *   each key also keeps its session peak, folded from the *closing* window
 *   inside the same rollover that clears the per-second value. No second timer,
 *   no per-render work, no allocation on the hot path.
 * - **Per-second numbers are not comparable across gestures.** One second of a
 *   slow drag and one second of a fling do very different amounts of work, so
 *   two runs with different gestures cannot be compared row by row.
 *   `recordScrollDistance` accumulates `|ΔscrollTop|` (called from the scroll
 *   pass with the delta it already computed; a no-op while off) and the
 *   snapshot divides the session totals by it — every cost then reads per
 *   1000 px scrolled, whatever the gesture was.
 * - **A suspended frame loop is not a frame gap.** `requestAnimationFrame`
 *   stops while the page is hidden (display asleep, window minimized, debugger
 *   paused), so the gap after it resumes measures the suspension, not the app.
 *   Gaps above `PERF_FRAME_SUSPENSION_MS` are counted separately and kept out of
 *   `frameGapMaxMs`; see that constant for why the threshold is where it is.
 */

/** localStorage flag: `'1'` = HUD on. Absent = off (the default). */
export const PERF_HUD_STORAGE_KEY = 'pointer.perfHud'
/** URL escape hatch: `?perfHud=1` / `?perfHud=0` (overrides storage on boot). */
export const PERF_HUD_URL_PARAM = 'perfHud'
/** Snapshot cadence of `useRenderPerfSnapshot()`. */
export const PERF_HUD_SNAPSHOT_MS = 250
/** Aggregation window for the per-second counters. */
export const PERF_HUD_WINDOW_MS = 1000
/**
 * Frame gap above which the loop is assumed to have been *suspended* rather than
 * the main thread blocked: the page was hidden (rAF stops), the display slept, or
 * a debugger paused the process. Such a gap is counted in `frameSuspensions` and
 * kept out of `frameGapMaxMs`.
 *
 * Why 5 s: a genuinely blocked main thread has to stay blocked for five whole
 * seconds to be misclassified, which no plausible synchronous render/mount pass
 * reaches (the counters here exist because individual passes cost tens of ms),
 * while every suspension above — backgrounded window, sleeping display,
 * breakpoint — produces gaps of many seconds to minutes.
 */
export const PERF_FRAME_SUSPENSION_MS = 5000
/** Human-readable shortcut, shown in the HUD header. */
export const PERF_HUD_SHORTCUT_LABEL = 'Cmd/Ctrl+Shift+Alt+P'

/** Instrumented components — kept here so the HUD can render zero rows. */
export const PERF_RENDER_KEYS = [
  'render:MessageList',
  'render:AssistantModelMessage',
  'render:SubAgentFrameHost',
  'render:SubAgentFrame',
  'render:SubAgentContentBlock'
] as const

export const PERF_MOUNT_KEYS = [
  'mount:MessageList',
  'mount:AssistantModelMessage',
  'mount:SubAgentFrameHost',
  'mount:SubAgentFrame',
  'mount:SubAgentContentBlock'
] as const

/** Duration of one coalesced `MessageList` scroll pass (see `runScrollPass`). */
export const PERF_MS_SCROLL_PASS = 'ms:scrollPass'

/** Accumulated milliseconds per second, keyed by `ms:<work>` (see `record`). */
export const PERF_MS_KEYS = [
  'ms:extraScopedForWindow',
  'ms:measureElement',
  'ms:toolRunAssistantMessage',
  PERF_MS_SCROLL_PASS
] as const

/** Per-second call count of `toolRunAssistantMessage` (fresh object per render). */
export const PERF_CALL_KEYS = ['call:toolRunAssistantMessage'] as const

export const PERF_GAUGE_VISIBLE_ROWS = 'visibleRows'
export const PERF_GAUGE_SCOPED_ROWS = 'scopedRows'
/** Entries in `MessageList`'s frozen per-turn file-changes cache. */
export const PERF_GAUGE_FROZEN_TURNS = 'frozenTurns'
/** Height the virtualizer computed for all rows (`getTotalSize()`). */
export const PERF_GAUGE_VIRTUAL_TOTAL = 'virtualTotal'
/** `scroller.scrollHeight` — the height the browser thinks the content has. */
export const PERF_GAUGE_VIRTUAL_SCROLL_HEIGHT = 'virtualScrollHeight'
/** First / last rendered row index; `-1` while nothing is rendered. */
export const PERF_GAUGE_VIRTUAL_FIRST = 'virtualFirst'
export const PERF_GAUGE_VIRTUAL_LAST = 'virtualLast'
/** Rows whose height was measured, as opposed to still reading `estimateSize`. */
export const PERF_GAUGE_VIRTUAL_MEASURED = 'virtualMeasured'
/**
 * `scrollHeight - getTotalSize()`. **Agrees by construction** — the scroll
 * container's content height is written from `getTotalSize()`, so this only ever
 * reports the surrounding padding (plus the pull spacer). Kept because a *stuck*
 * pull spacer still moves it; the per-row truth is `virtualSlack` / `virtualOverlap`.
 */
export const PERF_GAUGE_VIRTUAL_BLANK = 'virtualBlank'
/**
 * Σ max(0, assumed - actual) over the rendered rows: how many px of the window
 * the rows were placed lower than their real height needs, i.e. blank stripes
 * between rows (see `lib/virtualRowSlack.ts`).
 */
export const PERF_GAUGE_VIRTUAL_SLACK = 'virtualSlack'
/**
 * Σ max(0, actual - assumed) over the rendered rows: how many px of real row
 * height the assumed offsets do not account for, i.e. rows overlapping.
 */
export const PERF_GAUGE_VIRTUAL_OVERLAP = 'virtualOverlap'
/**
 * Height of the pull-to-load top spacer, which `MessageList` renders *inside* the
 * same scroll box as the rows. It is part of `virtualBlank` (together with the
 * scroller's own bottom padding), so publishing it is what decomposes a blank
 * region: a spacer left standing after an older-load reads as
 * `virtualBlank ≈ virtualPullSpacer` + padding, with every row still present.
 */
export const PERF_GAUGE_VIRTUAL_PULL_SPACER = 'virtualPullSpacer'

// --- activity markers -------------------------------------------------------

/**
 * Names of the heavy operations the HUD can blame a frame gap or a slow second
 * on. Fixed strings (never built at the call site), so attribution costs no
 * allocation — see `beginPerfActivity`.
 */
export const PERF_ACTIVITY_LAYOUT_REBUILD = 'layoutRebuild'
export const PERF_ACTIVITY_TRIM_HISTORY = 'trimHistory'
export const PERF_ACTIVITY_LOAD_OLDER = 'loadOlder'
export const PERF_ACTIVITY_LOAD_NEWER = 'loadNewer'
export const PERF_ACTIVITY_MEASURE_BATCH = 'measureBatch'
export const PERF_ACTIVITY_MARKDOWN_PARSE = 'markdownParse'
/**
 * Phases of `parseMarkdown`, nested inside `PERF_ACTIVITY_MARKDOWN_PARSE` so a
 * gap can still be blamed on the whole parse while the HUD shows which phase
 * held the time (source pre-processing, `marked.parse`, HTML post-pass).
 */
export const PERF_ACTIVITY_MARKDOWN_PARSE_PREP = 'markdownParse:prep'
export const PERF_ACTIVITY_MARKDOWN_PARSE_MARKED = 'markdownParse:marked'
export const PERF_ACTIVITY_MARKDOWN_PARSE_WRAP = 'markdownParse:wrap'
/** Shown instead of an activity name when no marker covers the moment. */
export const PERF_ACTIVITY_IDLE = 'idle'
/**
 * An activity still open after this long is assumed to have missed its `end`
 * and is dropped, so a bug in one call site cannot pin the HUD to a stale
 * operation for the rest of the session. Well above the slowest real operation
 * (the worst gap observed is ~0.75 s) and far below a session.
 */
export const PERF_ACTIVITY_STALE_MS = 5000

export interface RenderPerfSnapshot {
  enabled: boolean
  /** rAF frames observed in the trailing second. */
  fps: number
  /** Longest gap between consecutive frames in the trailing second. */
  frameGapMs: number
  /** Longest frame gap since the HUD was enabled; suspensions are excluded. */
  frameGapMaxMs: number
  /**
   * Operation that was running when `frameGapMaxMs` was recorded: the innermost
   * open activity, else the last activity to close since the previous frame,
   * else `''` — rendered as `idle` by the HUD. See `beginPerfActivity`.
   */
  frameGapMaxActivity: string
  /** Gaps above `PERF_FRAME_SUSPENSION_MS` skipped since the HUD was enabled. */
  frameSuspensions: number
  /** Per-second render counts, keyed `render:<Component>`. */
  renders: Record<string, number>
  /** Per-second mount counts, keyed `mount:<Component>`. */
  mounts: Record<string, number>
  /** Per-second call counts of instrumented helpers, keyed `call:<helper>`. */
  calls: Record<string, number>
  /** Accumulated milliseconds in the last completed second, keyed `ms:<work>`. */
  msPerSecond: Record<string, number>
  /** Latest gauge values (`visibleRows`, `scopedRows`, …). */
  gauges: Record<string, number>
  /**
   * Highest value each gauge reached since the HUD was toggled on, folded on
   * every `setGauge` rather than at the rollover: a spike that passes within a
   * frame or two (a transient blank stripe) is gone by the time the next second
   * closes, so the peak is the only trace a later reading can show.
   */
  peakGauges: Record<string, number>
  /** Sum of all `render:*` counters for the last completed second. */
  totalRendersPerSecond: number
  /** `totalRendersPerSecond / visibleRows` — repeated re-rendering at a glance. */
  rendersPerVisibleRow: number
  /**
   * Highest value each `renders` key reached in any completed second. Survives
   * the per-second reset, so a screenshot after a scroll still shows the cost.
   */
  peakRenders: Record<string, number>
  /** Highest value each `mounts` key reached in any completed second. */
  peakMounts: Record<string, number>
  /** Highest value each `calls` key reached in any completed second. */
  peakCalls: Record<string, number>
  /** Highest value each `msPerSecond` key reached in any completed second. */
  peakMsPerSecond: Record<string, number>
  /**
   * Operation that held the most time in the completed second which set
   * `peakMsPerSecond[PERF_MS_SCROLL_PASS]`; `''` when nothing was marked in that
   * second. See `beginPerfActivity`.
   */
  peakScrollPassActivity: string
  /** Highest `totalRendersPerSecond` seen in a completed second. */
  totalRendersPerSecondPeak: number
  /** Highest `rendersPerVisibleRow` seen at a rollover; 0 visible rows never raises it. */
  rendersPerVisibleRowPeak: number
  /**
   * Sum of `|ΔscrollTop|` over every scroll pass since the HUD was enabled
   * (see `recordScrollDistance`). The denominator of the per-1000-px costs; `0`
   * until the transcript window has actually moved.
   */
  scrollDistancePx: number
  /**
   * Session totals divided by `scrollDistancePx`, so two runs with different
   * gestures can be compared per 1000 px scrolled. All zero while no distance
   * has accumulated (never a division by zero).
   */
  per1000Px: RenderPerfCostPer1000Px
  /**
   * Latest frontend memory reading (see `lib/memoryProbe.ts`). `null` while the
   * HUD is off, or before the HUD installs its sampler.
   */
  memory: MemoryStats | null
}

/** Session cost per 1000 px of accumulated scroll distance (see `per1000Px`). */
export interface RenderPerfCostPer1000Px {
  /** Every `render:*` counter summed over the session, per 1000 px. */
  renders: number
  /** Every `mount:*` counter summed over the session, per 1000 px. */
  mounts: number
  /** `ms:measureElement` accumulated over the session, per 1000 px. */
  measureElementMs: number
  /** `ms:scrollPass` accumulated over the session, per 1000 px. */
  scrollPassMs: number
}

// --- module state -----------------------------------------------------------

let enabled = false
let initialized = false
const enabledListeners = new Set<(next: boolean) => void>()

/** `-1` = window starts at the next snapshot (see `resetPerf`). `performance.now()`
 *  can legitimately be `0`, so `0` would be an unsafe sentinel. */
let windowStartMs = -1
let counts = new Map<string, number>()
let msTotals = new Map<string, number>()
let closedCounts = new Map<string, number>()
let closedMs = new Map<string, number>()
const gauges = new Map<string, number>()
/**
 * Session peak per gauge, folded in `setGauge` so a value that spikes and returns
 * between two snapshots still leaves a trace (see `RenderPerfSnapshot.peakGauges`).
 */
const gaugePeaks = new Map<string, number>()
/**
 * Session peaks. Folded from each *closing* window by `foldPeaks`, so they cost
 * nothing per render and only grow until the HUD is toggled (see `resetPerf`).
 */
let peakCounts = new Map<string, number>()
let peakMs = new Map<string, number>()
let peakTotalRenders = 0
let peakRendersPerVisibleRow = 0
/**
 * Session totals, folded from each *closing* window right next to the peaks
 * (in place, at most once per second) — the numerator of the per-1000-px costs.
 */
let sessionTotals = new Map<string, number>()
/** Sum of `|ΔscrollTop|` reported by `recordScrollDistance` (see `per1000Px`). */
let scrollDistancePx = 0

/**
 * Innermost-first stack of open activities, with parallel start times used only
 * to drop a marker whose `end` never came. Both arrays are reused (push / pop /
 * in-place shift) so a marker allocates nothing per call.
 */
const activityStack: string[] = []
const activityStartMs: number[] = []
/** Open count per name, so an `end` without a `begin` stays a no-op. */
const activityCounts = new Map<string, number>()
/** Most recently closed activity — the one a frame gap is blamed on. */
let lastActivityName = ''
/** Set when an activity closes; cleared by the next frame callback. */
let activityEndedSinceFrame = false
/** Milliseconds each activity held in the current second (pass-peak attribution). */
const activityMsWindow = new Map<string, number>()
let frameGapMaxActivity = ''
let peakScrollPassActivity = ''

/** Fixed ring for frame timestamps — no per-frame allocation. */
const FRAME_RING = 600
const frameTimes = new Float64Array(FRAME_RING)
let frameWrite = 0
let frameTotal = 0
let hasLastFrame = false
let lastFrameTs = 0
let frameGapMaxMs = 0
let frameSuspensions = 0
let rafHandle: number | null = null

function nowMs(): number {
  if (typeof performance !== 'undefined' && typeof performance.now === 'function') {
    return performance.now()
  }
  return Date.now()
}

// --- counters ---------------------------------------------------------------

/** Single boolean gate every instrumentation point checks first. */
export function renderPerfEnabled(): boolean {
  return enabled
}

/** Count one occurrence of `name` in the current second. */
export function bump(name: string): void {
  if (!enabled) return
  counts.set(name, (counts.get(name) ?? 0) + 1)
}

/** Add `ms` of work attributed to `name` in the current second. */
export function record(name: string, ms: number): void {
  if (!enabled) return
  if (!Number.isFinite(ms) || ms < 0) return
  msTotals.set(name, (msTotals.get(name) ?? 0) + ms)
}

/** Latest point-in-time value (window size, row count, …). */
export function setGauge(name: string, value: number): void {
  if (!enabled) return
  const next = Number.isFinite(value) ? value : 0
  gauges.set(name, next)
  // Folded here, not at the rollover: a transient spike (a blank stripe that
  // passes in a frame or two) is gone by the time the next second closes, and its
  // peak is the only trace a later reading can show.
  if (next > (gaugePeaks.get(name) ?? 0)) gaugePeaks.set(name, next)
}

/**
 * Add `px` of scrolled distance (`|ΔscrollTop|`) to the session total that
 * normalises every cost per 1000 px. Called once per scroll pass with the delta
 * that pass already computed; while off it is one boolean read, like the other
 * instrumentation points.
 */
export function recordScrollDistance(px: number): void {
  if (!enabled) return
  if (!Number.isFinite(px)) return
  scrollDistancePx += Math.abs(px)
}

// --- virtualizer geometry ---------------------------------------------------

/**
 * Structural view of the row virtualizer. Typed structurally so this module
 * keeps no dependency on `@tanstack/virtual-core` (and tests need no virtualizer).
 */
export interface PerfVirtualizerLike {
  /** Content height the virtualizer computed from its measurements. */
  getTotalSize: () => number
  /** Cache of *measured* row heights (public in virtual-core 3.17.6); only `size` is read. */
  itemSizeCache?: { size: number } | null
}

/** Structural view of the scroll container — `scrollHeight` is the only read. */
export interface PerfScrollerLike {
  scrollHeight: number
}

/** One rendered row, as much as the gauge needs of it. */
export interface PerfVirtualRowLike {
  index: number
}

/**
 * Publish the virtualizer's geometry, so a *persistent* blank region can be told
 * apart from a missing-row bug:
 *
 * - `virtualTotal` against `virtualScrollHeight` — their difference
 *   (`virtualBlank`) is emptiness the scroller can still scroll into: every row
 *   is present, the box around them is too tall. (A missing row instead shrinks
 *   `virtualTotal`, and the two stay in step.)
 * - `virtualMeasured` against the rendered range — how many rows the offsets
 *   come from real heights rather than from `estimateSize`, which is what makes
 *   a freshly prepended page sit at a wrong offset.
 * - `virtualPullSpacer` — the pull-to-load spacer lives inside the same scroll
 *   box, so it is part of `virtualBlank`; when the two roughly match (plus the
 *   scroller's bottom padding) the blank region is that spacer, not lost rows.
 *
 * A missing virtualizer or scroller (before mount, after teardown, a null ref) is
 * a legal state, not an error: the gauges are published as `0` / `-1` rather than
 * throwing or leaving a stale reading from the previous conversation.
 *
 * While the HUD is off this is one boolean read, like every other point here.
 */
export function publishVirtualGeometryGauges(
  virtualizer: PerfVirtualizerLike | null | undefined,
  scroller: PerfScrollerLike | null | undefined,
  rows: readonly PerfVirtualRowLike[] | null | undefined,
  pullSpacerPx: number
): void {
  if (!enabled) return
  const totalSize = virtualizer ? virtualizer.getTotalSize() : 0
  const scrollHeight = scroller ? scroller.scrollHeight : 0
  const hasRows = !!rows && rows.length > 0
  const firstRow = rows && hasRows ? rows[0] : undefined
  const lastRow = rows && hasRows ? rows[rows.length - 1] : undefined
  setGauge(PERF_GAUGE_VIRTUAL_TOTAL, totalSize)
  setGauge(PERF_GAUGE_VIRTUAL_SCROLL_HEIGHT, scrollHeight)
  setGauge(PERF_GAUGE_VIRTUAL_FIRST, firstRow?.index ?? -1)
  setGauge(PERF_GAUGE_VIRTUAL_LAST, lastRow?.index ?? -1)
  setGauge(PERF_GAUGE_VIRTUAL_MEASURED, virtualizer?.itemSizeCache?.size ?? 0)
  // Read from the caller's own state (a ref), not from the DOM: the spacer is a
  // height the component renders, and a negative one is meaningless.
  setGauge(PERF_GAUGE_VIRTUAL_PULL_SPACER, pullSpacerPx > 0 ? pullSpacerPx : 0)
  // Only meaningful with both sides present: a missing scroller would otherwise
  // read as a large negative "blank".
  setGauge(
    PERF_GAUGE_VIRTUAL_BLANK,
    virtualizer && scroller ? scrollHeight - totalSize : 0
  )
}

/**
 * Publish the per-row deviation summed by `measureRenderedRowSlack`
 * (`lib/virtualRowSlack.ts`), which is the gauge pair that actually shows the
 * blank stripes: `virtualBlank` above agrees by construction and cannot.
 *
 * Both sums are built from `max(0, …)`, so a negative here can only come from a
 * caller bug; it is clamped rather than published, the same way the pull spacer is.
 */
export function publishVirtualRowSlackGauges(slackPx: number, overlapPx: number): void {
  if (!enabled) return
  setGauge(PERF_GAUGE_VIRTUAL_SLACK, slackPx > 0 ? slackPx : 0)
  setGauge(PERF_GAUGE_VIRTUAL_OVERLAP, overlapPx > 0 ? overlapPx : 0)
}

/**
 * Mark the start of a heavy operation so a frame gap or a slow second can be
 * blamed on it instead of on the app in general. Pair with `endPerfActivity`.
 *
 * Contract:
 *
 * - **Free while the HUD is off** — one boolean read, like `bump` / `record`.
 * - **Nesting-safe** — `begin` / `end` pair up per name and the innermost open
 *   activity is the one reported.
 * - **Never throws, never goes negative** — an `end` without a `begin` is
 *   ignored, and an activity whose `end` never comes is dropped after
 *   `PERF_ACTIVITY_STALE_MS` (see `pruneStaleActivities`).
 * - **No allocation per call** — the name is pushed on a reused array and its
 *   ref count is an existing `Map` entry.
 *
 * Call sites read `renderPerfEnabled()` once and branch, like every other
 * instrumentation point, so nothing here needs a closure.
 */
export function beginPerfActivity(name: string): void {
  if (!enabled || !name) return
  activityStack.push(name)
  activityStartMs.push(nowMs())
  activityCounts.set(name, (activityCounts.get(name) ?? 0) + 1)
}

/**
 * Close the innermost open `name`.
 *
 * The elapsed time also lands in the current second's activity totals, which is
 * what attributes a slow `ms:scrollPass` second to an operation.
 */
export function endPerfActivity(name: string): void {
  if (!enabled || !name) return
  const open = activityCounts.get(name) ?? 0
  // `end` without `begin`, or a double `end`: ignore rather than corrupt the
  // stack or drive the count negative.
  if (open <= 0) return
  if (open === 1) activityCounts.delete(name)
  else activityCounts.set(name, open - 1)

  let index = -1
  for (let i = activityStack.length - 1; i >= 0; i -= 1) {
    if (activityStack[i] === name) {
      index = i
      break
    }
  }
  if (index < 0) return

  const startedAt = activityStartMs[index] ?? 0
  const endedAt = nowMs()
  // In-place shift: `splice` would allocate a result array on every call.
  for (let i = index; i < activityStack.length - 1; i += 1) {
    activityStack[i] = activityStack[i + 1] ?? ''
    activityStartMs[i] = activityStartMs[i + 1] ?? 0
  }
  activityStack.pop()
  activityStartMs.pop()

  lastActivityName = name
  activityEndedSinceFrame = true
  const spent = endedAt - startedAt
  if (spent > 0) activityMsWindow.set(name, (activityMsWindow.get(name) ?? 0) + spent)
}

/** Innermost open activity, or `''` when none is marked. */
export function currentPerfActivity(): string {
  return activityStack.length > 0 ? activityStack[activityStack.length - 1] ?? '' : ''
}

/**
 * Drop activities open longer than `PERF_ACTIVITY_STALE_MS`. A call site that
 * missed its `end` (threw, early-returned, forgot) must not leave the HUD
 * blaming an operation that finished minutes ago. Runs once per frame while
 * enabled, so the bound needs no timer of its own.
 */
function pruneStaleActivities(now: number): void {
  for (let i = activityStack.length - 1; i >= 0; i -= 1) {
    if (now - (activityStartMs[i] ?? now) <= PERF_ACTIVITY_STALE_MS) continue
    const name = activityStack[i] ?? ''
    for (let j = i; j < activityStack.length - 1; j += 1) {
      activityStack[j] = activityStack[j + 1] ?? ''
      activityStartMs[j] = activityStartMs[j + 1] ?? 0
    }
    activityStack.pop()
    activityStartMs.pop()
    const open = activityCounts.get(name) ?? 0
    if (open <= 1) activityCounts.delete(name)
    else activityCounts.set(name, open - 1)
  }
}

/**
 * Operation to blame for a frame gap detected now.
 *
 * A gap is noticed *after* the blocking work has finished — that is what frees
 * the frame callback — so a synchronous operation has already closed by then.
 * The "closed since the previous frame" flag is therefore what attributes it;
 * an activity still open covers the async paths (`loadOlder` / `loadNewer`).
 */
function attributeFrameGapActivity(): string {
  if (activityStack.length > 0) return activityStack[activityStack.length - 1] ?? ''
  return activityEndedSinceFrame ? lastActivityName : ''
}

/** Activity holding the most time in the window that is closing. */
function dominantActivityOfWindow(): string {
  let best = ''
  let bestMs = 0
  for (const [name, ms] of activityMsWindow) {
    if (ms > bestMs) {
      bestMs = ms
      best = name
    }
  }
  return best
}

/** Drop all accumulated counters, gauges, session peaks and frame samples. */
export function resetPerf(): void {
  counts = new Map()
  msTotals = new Map()
  closedCounts = new Map()
  closedMs = new Map()
  gauges.clear()
  gaugePeaks.clear()
  peakCounts = new Map()
  peakMs = new Map()
  peakTotalRenders = 0
  peakRendersPerVisibleRow = 0
  sessionTotals = new Map()
  scrollDistancePx = 0
  activityStack.length = 0
  activityStartMs.length = 0
  activityCounts.clear()
  lastActivityName = ''
  activityEndedSinceFrame = false
  activityMsWindow.clear()
  frameGapMaxActivity = ''
  peakScrollPassActivity = ''
  frameWrite = 0
  frameTotal = 0
  hasLastFrame = false
  lastFrameTs = 0
  frameGapMaxMs = 0
  frameSuspensions = 0
  windowStartMs = -1
}

// --- frame probe ------------------------------------------------------------

function frameLoopAvailable(): boolean {
  return typeof requestAnimationFrame === 'function' && typeof cancelAnimationFrame === 'function'
}

function onFrame(ts: number): void {
  rafHandle = requestAnimationFrame(onFrame)
  // Bound a marker whose `end` never came (see pruneStaleActivities) before the
  // gap below is attributed to whatever is still open.
  pruneStaleActivities(nowMs())
  if (hasLastFrame) {
    const gap = ts - lastFrameTs
    if (gap > PERF_FRAME_SUSPENSION_MS) {
      // The loop was stopped (hidden page / sleeping display / paused debugger).
      // Keep it out of the maximum so a suspension never reads as a frame stall.
      frameSuspensions++
    } else if (gap > frameGapMaxMs) {
      frameGapMaxMs = gap
      frameGapMaxActivity = attributeFrameGapActivity()
    }
  }
  // Every frame consumes the "an activity closed since the last frame" flag, so
  // a gap is only ever blamed on work done inside that one inter-frame window.
  activityEndedSinceFrame = false
  hasLastFrame = true
  lastFrameTs = ts
  frameTimes[frameWrite] = ts
  frameWrite = (frameWrite + 1) % FRAME_RING
  frameTotal++
}

function startFrameLoop(): void {
  if (rafHandle !== null || !frameLoopAvailable()) return
  rafHandle = requestAnimationFrame(onFrame)
}

function stopFrameLoop(): void {
  if (rafHandle === null) return
  cancelAnimationFrame(rafHandle)
  rafHandle = null
}

function readFrameStats(t: number): { fps: number; frameGapMs: number } {
  const valid = Math.min(frameTotal, FRAME_RING)
  let fps = 0
  let longest = 0
  let newer = -1
  for (let back = 1; back <= valid; back++) {
    const ts = frameTimes[(frameWrite - back + FRAME_RING) % FRAME_RING]
    if (t - ts > PERF_HUD_WINDOW_MS) break
    fps++
    if (newer >= 0) {
      const gap = newer - ts
      // A suspension inside the window is not a frame gap either (see onFrame).
      if (gap <= PERF_FRAME_SUSPENSION_MS && gap > longest) longest = gap
    }
    newer = ts
  }
  return { fps, frameGapMs: longest }
}

// --- memory sampler ---------------------------------------------------------

/**
 * Optional frontend memory sampler, installed by the HUD (`lib/memoryProbe.ts`).
 * It is only invoked while `enabled`, so an installed-but-off probe never reads
 * the DOM or any cache.
 */
let memorySampler: ((nowMs: number) => MemoryStats) | null = null

export function setRenderPerfMemorySampler(
  sampler: ((nowMs: number) => MemoryStats) | null
): void {
  memorySampler = sampler
}

// --- gauge sampler ----------------------------------------------------------

/**
 * Optional virtualizer-geometry sampler, installed by `MessageList` (see
 * `publishVirtualGeometryGauges`). Like the memory sampler it is invoked only
 * while `enabled` — and it rides the existing snapshot tick instead of a timer
 * of its own, so the read (which includes `scrollHeight`, a layout-forcing one)
 * happens at the HUD's 4 Hz cadence rather than once per component update.
 */
let gaugeSampler: (() => void) | null = null

/** Low-level slot, mirroring `setRenderPerfMemorySampler` (tests, one sampler at a time). */
export function setRenderPerfGaugeSampler(sampler: (() => void) | null): void {
  gaugeSampler = sampler
}

/**
 * Install a geometry sampler and return its disposer. The disposer clears the
 * slot only while this sampler is still the installed one, so a component that
 * unmounts after a newer one registered cannot unsubscribe its successor.
 */
export function installRenderPerfGaugeSampler(sampler: () => void): () => void {
  gaugeSampler = sampler
  return () => {
    if (gaugeSampler === sampler) gaugeSampler = null
  }
}

// --- snapshot ---------------------------------------------------------------

/**
 * Fold one *closing* window into the session peaks and the session totals. Runs
 * at most once per second (from the rollover in `renderPerfSnapshot`), so both
 * add no timer and no per-render work; `rendersPerVisibleRow` only folds while
 * rows are visible, so an empty transcript cannot drag the ratio's peak to zero.
 */
function foldPeaks(windowCounts: Map<string, number>, windowMs: Map<string, number>): void {
  let windowRenders = 0
  for (const [name, value] of windowCounts) {
    if (value <= 0) continue
    if (value > (peakCounts.get(name) ?? 0)) peakCounts.set(name, value)
    sessionTotals.set(name, (sessionTotals.get(name) ?? 0) + value)
    if (name.startsWith('render:')) windowRenders += value
  }
  for (const [name, value] of windowMs) {
    if (value <= 0) continue
    if (value > (peakMs.get(name) ?? 0)) {
      peakMs.set(name, value)
      // The pass peak is what a post-scroll screenshot shows, so record which
      // operation held that second — the window's totals are still intact here.
      if (name === PERF_MS_SCROLL_PASS) peakScrollPassActivity = dominantActivityOfWindow()
    }
    sessionTotals.set(name, (sessionTotals.get(name) ?? 0) + value)
  }
  if (windowRenders > peakTotalRenders) peakTotalRenders = windowRenders
  const visibleRows = gauges.get(PERF_GAUGE_VISIBLE_ROWS) ?? 0
  if (visibleRows > 0 && windowRenders / visibleRows > peakRendersPerVisibleRow) {
    peakRendersPerVisibleRow = windowRenders / visibleRows
  }
}

/**
 * Session total → cost per 1000 px scrolled. A session that has not scrolled
 * (distance `0`, or not yet known) reads `0` rather than dividing by zero.
 */
function perThousandPx(total: number, distancePx: number): number {
  if (!(distancePx > 0)) return 0
  return (total * 1000) / distancePx
}

/**
 * Build a snapshot, closing the current second when it has elapsed.
 *
 * `nowOverride` exists so tests can drive the aggregation deterministically
 * without faking `performance.now`.
 */
export function renderPerfSnapshot(nowOverride?: number): RenderPerfSnapshot {
  const t = nowOverride ?? nowMs()
  if (windowStartMs < 0) {
    windowStartMs = t
  } else if (t - windowStartMs >= PERF_HUD_WINDOW_MS) {
    foldPeaks(counts, msTotals)
    closedCounts = counts
    closedMs = msTotals
    counts = new Map()
    msTotals = new Map()
    // The window's activity totals are only read by `foldPeaks` above; start the
    // next second's attribution from empty.
    activityMsWindow.clear()
    windowStartMs = t
  }

  const frames = readFrameStats(t)
  const renders: Record<string, number> = {}
  const mounts: Record<string, number> = {}
  const calls: Record<string, number> = {}
  const msPerSecond: Record<string, number> = {}
  let totalRenders = 0
  for (const [name, value] of closedCounts) {
    if (value <= 0) continue
    if (name.startsWith('render:')) {
      renders[name] = value
      totalRenders += value
    } else if (name.startsWith('mount:')) {
      mounts[name] = value
    } else if (name.startsWith('call:')) {
      calls[name] = value
    }
  }
  for (const [name, value] of closedMs) {
    if (value > 0) msPerSecond[name] = value
  }
  // Refresh the live geometry gauges once per snapshot — never per update: the
  // read includes `scrollHeight`, which forces layout when the DOM is dirty.
  if (enabled && gaugeSampler) gaugeSampler()
  const gaugeSnapshot: Record<string, number> = {}
  for (const [name, value] of gauges) gaugeSnapshot[name] = value
  const peakGaugeSnapshot: Record<string, number> = {}
  for (const [name, value] of gaugePeaks) peakGaugeSnapshot[name] = value

  // Peaks are rebuilt with the snapshot (4 Hz), exactly like the other records.
  const peakRenders: Record<string, number> = {}
  const peakMounts: Record<string, number> = {}
  const peakCalls: Record<string, number> = {}
  for (const [name, value] of peakCounts) {
    if (name.startsWith('render:')) peakRenders[name] = value
    else if (name.startsWith('mount:')) peakMounts[name] = value
    else if (name.startsWith('call:')) peakCalls[name] = value
  }
  const peakMsPerSecond: Record<string, number> = {}
  for (const [name, value] of peakMs) peakMsPerSecond[name] = value

  const visibleRows = gauges.get(PERF_GAUGE_VISIBLE_ROWS) ?? 0

  // Session totals → per-1000-px costs. Derived here, once per snapshot, so the
  // scroll pass only ever adds the distance and nothing stores a ratio.
  let sessionRenders = 0
  let sessionMounts = 0
  for (const [name, value] of sessionTotals) {
    if (name.startsWith('render:')) sessionRenders += value
    else if (name.startsWith('mount:')) sessionMounts += value
  }
  const per1000Px: RenderPerfCostPer1000Px = {
    renders: perThousandPx(sessionRenders, scrollDistancePx),
    mounts: perThousandPx(sessionMounts, scrollDistancePx),
    measureElementMs: perThousandPx(sessionTotals.get('ms:measureElement') ?? 0, scrollDistancePx),
    scrollPassMs: perThousandPx(sessionTotals.get(PERF_MS_SCROLL_PASS) ?? 0, scrollDistancePx)
  }

  return {
    enabled,
    fps: frames.fps,
    frameGapMs: frames.frameGapMs,
    frameGapMaxMs,
    frameGapMaxActivity,
    frameSuspensions,
    renders,
    mounts,
    calls,
    msPerSecond,
    gauges: gaugeSnapshot,
    peakGauges: peakGaugeSnapshot,
    totalRendersPerSecond: totalRenders,
    rendersPerVisibleRow: visibleRows > 0 ? totalRenders / visibleRows : 0,
    peakRenders,
    peakMounts,
    peakCalls,
    peakMsPerSecond,
    peakScrollPassActivity,
    totalRendersPerSecondPeak: peakTotalRenders,
    rendersPerVisibleRowPeak: peakRendersPerVisibleRow,
    scrollDistancePx,
    per1000Px,
    memory: enabled && memorySampler ? memorySampler(t) : null
  }
}

// --- toggle sources ---------------------------------------------------------

function readStoredPreference(): boolean {
  try {
    return localStorage.getItem(PERF_HUD_STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

function writeStoredPreference(next: boolean): void {
  try {
    if (next) localStorage.setItem(PERF_HUD_STORAGE_KEY, '1')
    else localStorage.removeItem(PERF_HUD_STORAGE_KEY)
  } catch {
    /* ignore quota / private mode */
  }
}

/** `?perfHud=1` → true, `?perfHud=0|false|off` → false, absent → null. */
export function parsePerfHudUrlFlag(search: string): boolean | null {
  const raw = search.startsWith('?') ? search.slice(1) : search
  if (!raw) return null
  const params = new URLSearchParams(raw)
  if (!params.has(PERF_HUD_URL_PARAM)) return null
  const value = (params.get(PERF_HUD_URL_PARAM) ?? '').trim().toLowerCase()
  if (value === '0' || value === 'false' || value === 'off') return false
  return true
}

/** URL wins over storage; storage is the persistent baseline. */
export function resolveInitialPerfHudEnabled(input: {
  stored: boolean
  urlFlag: boolean | null
}): boolean {
  return input.urlFlag !== null ? input.urlFlag : input.stored
}

/** `Cmd+Shift+Alt+P` on macOS, `Ctrl+Shift+Alt+P` elsewhere. */
export function isPerfHudShortcut(
  event: Pick<KeyboardEvent, 'key' | 'code' | 'altKey' | 'shiftKey' | 'metaKey' | 'ctrlKey'>
): boolean {
  if (!event.altKey || !event.shiftKey) return false
  if (!event.metaKey && !event.ctrlKey) return false
  // Option+P rewrites `key` on macOS layouts (e.g. 'π'), so accept the code too.
  return event.code === 'KeyP' || event.key.toLowerCase() === 'p'
}

export function onRenderPerfEnabledChange(listener: (next: boolean) => void): () => void {
  enabledListeners.add(listener)
  return () => {
    enabledListeners.delete(listener)
  }
}

/**
 * Turn the HUD on/off. Persists to localStorage by default (the keyboard and
 * console toggles are user intent; boot-time resolution is not).
 */
export function setRenderPerfEnabled(next: boolean, options?: { persist?: boolean }): void {
  const value = next === true
  if (options?.persist !== false) writeStoredPreference(value)
  if (value === enabled) return
  enabled = value
  if (value) {
    resetPerf()
    startFrameLoop()
  } else {
    stopFrameLoop()
    resetPerf()
  }
  for (const listener of [...enabledListeners]) listener(value)
}

export function toggleRenderPerf(): boolean {
  setRenderPerfEnabled(!enabled)
  return enabled
}

function onPerfHudKeydown(event: KeyboardEvent): void {
  if (!isPerfHudShortcut(event)) return
  event.preventDefault()
  event.stopPropagation()
  toggleRenderPerf()
}

declare global {
  interface Window {
    /** Console escape hatch for builds without DevTools: `__pointerPerfHud(true)`. */
    __pointerPerfHud?: (next?: boolean) => boolean
  }
}

/**
 * Install the global toggle sources. Idempotent — call once from app bootstrap
 * so the shortcut works before (and while) the HUD is off.
 */
export function initRenderPerf(): void {
  if (initialized) return
  initialized = true

  const search = typeof window !== 'undefined' ? (window.location?.search ?? '') : ''
  const urlFlag = parsePerfHudUrlFlag(search)
  const initial = resolveInitialPerfHudEnabled({ stored: readStoredPreference(), urlFlag })
  if (initial) setRenderPerfEnabled(true, { persist: false })

  if (typeof window === 'undefined') return
  window.addEventListener('keydown', onPerfHudKeydown, true)
  window.__pointerPerfHud = (next?: boolean) => {
    if (typeof next === 'boolean') setRenderPerfEnabled(next)
    else toggleRenderPerf()
    return enabled
  }
}

// --- Vue bindings -----------------------------------------------------------

/** Reactive HUD state, refreshed at `PERF_HUD_SNAPSHOT_MS` while enabled. */
export function useRenderPerfSnapshot(): Ref<RenderPerfSnapshot> {
  const snapshot = ref<RenderPerfSnapshot>(renderPerfSnapshot())
  let timer: ReturnType<typeof setInterval> | null = null

  const stop = () => {
    if (timer === null) return
    clearInterval(timer)
    timer = null
  }
  const start = () => {
    stop()
    timer = setInterval(() => {
      snapshot.value = renderPerfSnapshot()
    }, PERF_HUD_SNAPSHOT_MS)
  }

  const unsubscribe = onRenderPerfEnabledChange(next => {
    if (next) start()
    else {
      stop()
      snapshot.value = renderPerfSnapshot()
    }
  })
  if (renderPerfEnabled()) start()

  if (getCurrentScope()) {
    onScopeDispose(() => {
      stop()
      unsubscribe()
    })
  }
  return snapshot
}

/** Reactive mirror of `renderPerfEnabled()` for `v-if` in the app shell. */
export function useRenderPerfEnabled(): Ref<boolean> {
  const on = ref(renderPerfEnabled())
  const unsubscribe = onRenderPerfEnabledChange(next => {
    on.value = next
  })
  if (getCurrentScope()) onScopeDispose(unsubscribe)
  return on
}
