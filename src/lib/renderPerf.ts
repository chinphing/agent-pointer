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
 *   not exist. The only cost while off is the (single) keydown listener.
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

export interface RenderPerfSnapshot {
  enabled: boolean
  /** rAF frames observed in the trailing second. */
  fps: number
  /** Longest gap between consecutive frames in the trailing second. */
  frameGapMs: number
  /** Longest frame gap since the HUD was enabled; suspensions are excluded. */
  frameGapMaxMs: number
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
  gauges.set(name, Number.isFinite(value) ? value : 0)
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

/** Drop all accumulated counters, gauges, session peaks and frame samples. */
export function resetPerf(): void {
  counts = new Map()
  msTotals = new Map()
  closedCounts = new Map()
  closedMs = new Map()
  gauges.clear()
  peakCounts = new Map()
  peakMs = new Map()
  peakTotalRenders = 0
  peakRendersPerVisibleRow = 0
  sessionTotals = new Map()
  scrollDistancePx = 0
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
  if (hasLastFrame) {
    const gap = ts - lastFrameTs
    if (gap > PERF_FRAME_SUSPENSION_MS) {
      // The loop was stopped (hidden page / sleeping display / paused debugger).
      // Keep it out of the maximum so a suspension never reads as a frame stall.
      frameSuspensions++
    } else if (gap > frameGapMaxMs) {
      frameGapMaxMs = gap
    }
  }
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
    if (value > (peakMs.get(name) ?? 0)) peakMs.set(name, value)
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
  const gaugeSnapshot: Record<string, number> = {}
  for (const [name, value] of gauges) gaugeSnapshot[name] = value

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
    frameSuspensions,
    renders,
    mounts,
    calls,
    msPerSecond,
    gauges: gaugeSnapshot,
    totalRendersPerSecond: totalRenders,
    rendersPerVisibleRow: visibleRows > 0 ? totalRenders / visibleRows : 0,
    peakRenders,
    peakMounts,
    peakCalls,
    peakMsPerSecond,
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
