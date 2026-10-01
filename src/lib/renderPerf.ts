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
 */

/** localStorage flag: `'1'` = HUD on. Absent = off (the default). */
export const PERF_HUD_STORAGE_KEY = 'pointer.perfHud'
/** URL escape hatch: `?perfHud=1` / `?perfHud=0` (overrides storage on boot). */
export const PERF_HUD_URL_PARAM = 'perfHud'
/** Snapshot cadence of `useRenderPerfSnapshot()`. */
export const PERF_HUD_SNAPSHOT_MS = 250
/** Aggregation window for the per-second counters. */
export const PERF_HUD_WINDOW_MS = 1000
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

/** Accumulated milliseconds per second, keyed by `ms:<work>` (see `record`). */
export const PERF_MS_KEYS = [
  'ms:extraScopedForWindow',
  'ms:measureElement',
  'ms:toolRunAssistantMessage'
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
  /** Longest frame gap since the HUD was enabled. */
  frameGapMaxMs: number
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
   * Latest frontend memory reading (see `lib/memoryProbe.ts`). `null` while the
   * HUD is off, or before the HUD installs its sampler.
   */
  memory: MemoryStats | null
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

/** Fixed ring for frame timestamps — no per-frame allocation. */
const FRAME_RING = 600
const frameTimes = new Float64Array(FRAME_RING)
let frameWrite = 0
let frameTotal = 0
let hasLastFrame = false
let lastFrameTs = 0
let frameGapMaxMs = 0
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

/** Drop all accumulated counters, gauges and frame samples. */
export function resetPerf(): void {
  counts = new Map()
  msTotals = new Map()
  closedCounts = new Map()
  closedMs = new Map()
  gauges.clear()
  frameWrite = 0
  frameTotal = 0
  hasLastFrame = false
  lastFrameTs = 0
  frameGapMaxMs = 0
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
    if (gap > frameGapMaxMs) frameGapMaxMs = gap
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
      if (gap > longest) longest = gap
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

  const visibleRows = gauges.get(PERF_GAUGE_VISIBLE_ROWS) ?? 0
  return {
    enabled,
    fps: frames.fps,
    frameGapMs: frames.frameGapMs,
    frameGapMaxMs,
    renders,
    mounts,
    calls,
    msPerSecond,
    gauges: gaugeSnapshot,
    totalRendersPerSecond: totalRenders,
    rendersPerVisibleRow: visibleRows > 0 ? totalRenders / visibleRows : 0,
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
