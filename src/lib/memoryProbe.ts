import { setRenderPerfMemorySampler } from './renderPerf'

/**
 * Dev-only frontend memory probe, surfaced by `components/dev/RenderPerfHud.vue`.
 *
 * Why it is shaped like this:
 *
 * - **Frontend only.** Nothing here reaches the Tauri/IPC layer: no process RSS,
 *   no `ps` probe. The signals are the webview's own heap (when it exposes one),
 *   the DOM node count, and the app-side counters the caller injects.
 * - **Off is free.** `installMemoryProbe()` is called from the HUD's `setup`, so
 *   the probe does not exist while the HUD is off, and `renderPerfSnapshot()`
 *   only invokes the sampler while `renderPerfEnabled()`. No DOM walk, no cache
 *   read, no allocation.
 * - **One tick, one second.** The probe rides the existing 4 Hz snapshot tick and
 *   throttles itself to `MEMORY_PROBE_INTERVAL_MS` instead of owning a second timer.
 * - **`n/a` is a value.** `performance.memory` is Chromium-only and absent on the
 *   macOS WKWebView build, so an unavailable reading must degrade to `n/a` rather
 *   than be hidden or reported as `0`.
 * - **Sources are injected.** The DOM/heap readers are pure; the app-side counters
 *   come from the caller so this module never imports a store (and its tests need
 *   neither Pinia nor the chat store).
 */

/** Sampling cadence — at most one source read per second, only while the HUD is on. */
export const MEMORY_PROBE_INTERVAL_MS = 1000

/** One tracked value with its session peak and its delta from the first reading. */
export interface MemoryMetric {
  /** Latest reading; `null` when the source is unavailable (e.g. no `performance.memory`). */
  current: number | null
  /** Highest reading since the probe started; survives unavailable samples. */
  peak: number | null
  /** First available reading since the probe started — `delta` is measured from here. */
  baseline: number | null
  /** `current - baseline`; `null` when either side is unavailable. */
  delta: number | null
}

/** One app-side counter, e.g. `{ label: 'lead msgs', value: 42 }`. */
export interface MemoryGaugeReading {
  /** Row label shown in the HUD. */
  label: string
  value: number
}

export interface MemoryHeapReading {
  usedBytes: number
  totalBytes: number
}

/** Everything the probe reads, injected so tests need no DOM, stores or webview. */
export interface MemoryProbeSources {
  /** `document.getElementsByTagName('*').length` — a live collection length, not a copy. */
  readDomNodes: () => number
  /** `performance.memory` when the webview exposes it (Chromium); `null` otherwise. */
  readHeap: () => MemoryHeapReading | null
  /** Cheap app-side counters (transcript length, cache entry counts, …). */
  readGauges: () => MemoryGaugeReading[]
}

export interface MemoryGaugeStat {
  label: string
  metric: MemoryMetric
}

export interface MemoryStats {
  /** `performance.memory.usedJSHeapSize` in bytes. */
  heapUsed: MemoryMetric
  /** `performance.memory.totalJSHeapSize` in bytes — the heap row's denominator. */
  heapTotalBytes: number | null
  /** `document.getElementsByTagName('*').length`. */
  domNodes: MemoryMetric
  /** App-side counters, in the order the caller reported them. */
  gauges: MemoryGaugeStat[]
}

export interface MemoryProbe {
  /**
   * Read the sources, at most once per `intervalMs`; between reads the previous
   * stats are replayed unchanged (so the HUD can call this on every tick).
   */
  sample: (nowMs: number) => MemoryStats
  /** Drop peaks and the baseline so the next sample starts a fresh session. */
  reset: () => void
}

export interface MemoryProbeHandle {
  dispose: () => void
}

// --- formatting -------------------------------------------------------------

const MB = 1024 * 1024

/** `n/a` when unavailable, otherwise one-decimal megabytes. */
export function formatBytes(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return 'n/a'
  return `${(value / MB).toFixed(1)} MB`
}

/** `n/a` when unavailable, otherwise a plain rounded count. */
export function formatCount(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return 'n/a'
  return String(Math.round(value))
}

/**
 * Signed delta through the same formatter as its row, so `Δ` reads like the value
 * it belongs to: `+12.3 MB`, `-4.0 MB`, `0` for a flat reading, `n/a` when unknown.
 */
export function formatDelta(
  value: number | null,
  format: (value: number | null) => string
): string {
  if (value === null || !Number.isFinite(value)) return 'n/a'
  if (value > 0) return `+${format(value)}`
  if (value < 0) return `-${format(-value)}`
  return format(0)
}

// --- metric tracking --------------------------------------------------------

/**
 * Fold one reading into a metric: `current` replaces, `peak` only grows, and
 * `baseline` is the first reading that was actually available. An unavailable
 * reading (`null`) never resets the peak or the baseline — it only blanks
 * `current`/`delta`, so the HUD shows `n/a` instead of a fabricated `0`.
 */
export function advanceMetric(
  previous: MemoryMetric | undefined,
  value: number | null
): MemoryMetric {
  if (value === null || !Number.isFinite(value)) {
    return {
      current: null,
      peak: previous?.peak ?? null,
      baseline: previous?.baseline ?? null,
      delta: null
    }
  }
  const baseline = previous?.baseline ?? value
  const peak = previous?.peak === null || previous?.peak === undefined
    ? value
    : Math.max(previous.peak, value)
  return { current: value, peak, baseline, delta: value - baseline }
}

// --- probe ------------------------------------------------------------------

/** Shared "no reading yet" metric — frozen so a consumer cannot corrupt it. */
const EMPTY_METRIC: MemoryMetric = Object.freeze({
  current: null,
  peak: null,
  baseline: null,
  delta: null
})

export function createMemoryProbe(
  sources: MemoryProbeSources,
  options?: { intervalMs?: number }
): MemoryProbe {
  const intervalMs = options?.intervalMs ?? MEMORY_PROBE_INTERVAL_MS
  /** `-Infinity` so the first `sample()` always reads (a real `0` clock is legal). */
  let lastSampleAtMs = Number.NEGATIVE_INFINITY
  let heapUsed: MemoryMetric | undefined
  let domNodes: MemoryMetric | undefined
  let heapTotalBytes: number | null = null
  const gaugeMetrics = new Map<string, MemoryMetric>()

  function stats(): MemoryStats {
    return {
      heapUsed: heapUsed ?? EMPTY_METRIC,
      heapTotalBytes,
      domNodes: domNodes ?? EMPTY_METRIC,
      gauges: [...gaugeMetrics].map(([label, metric]) => ({ label, metric }))
    }
  }

  function reset(): void {
    lastSampleAtMs = Number.NEGATIVE_INFINITY
    heapUsed = undefined
    domNodes = undefined
    heapTotalBytes = null
    gaugeMetrics.clear()
  }

  function sample(nowMs: number): MemoryStats {
    if (nowMs - lastSampleAtMs < intervalMs) return stats()
    lastSampleAtMs = nowMs

    const heap = sources.readHeap()
    heapUsed = advanceMetric(heapUsed, heap?.usedBytes ?? null)
    heapTotalBytes = heap?.totalBytes ?? null
    domNodes = advanceMetric(domNodes, sources.readDomNodes())

    const readings = sources.readGauges()
    const seen = new Set<string>()
    for (const reading of readings) {
      if (seen.has(reading.label)) continue
      seen.add(reading.label)
      gaugeMetrics.set(reading.label, advanceMetric(gaugeMetrics.get(reading.label), reading.value))
    }
    // A counter that stopped being reported must not linger as a stale row.
    for (const label of [...gaugeMetrics.keys()]) {
      if (!seen.has(label)) gaugeMetrics.delete(label)
    }

    return stats()
  }

  return { sample, reset }
}

// --- default readers --------------------------------------------------------

interface PerformanceMemoryLike {
  usedJSHeapSize: number
  totalJSHeapSize: number
}

/** `performance.memory` — Chromium-only, so `null` on WKWebView (macOS Tauri build). */
export function readJsHeap(): MemoryHeapReading | null {
  if (typeof performance === 'undefined') return null
  const memory = (performance as unknown as { memory?: PerformanceMemoryLike }).memory
  if (!memory) return null
  if (typeof memory.usedJSHeapSize !== 'number' || typeof memory.totalJSHeapSize !== 'number') {
    return null
  }
  return { usedBytes: memory.usedJSHeapSize, totalBytes: memory.totalJSHeapSize }
}

/** Live `HTMLCollection` length — no array copy of every node. */
export function readDomNodeCount(): number {
  if (typeof document === 'undefined') return 0
  return document.getElementsByTagName('*').length
}

// --- installation -----------------------------------------------------------

/** The probe currently registered with `renderPerf` (one HUD at a time). */
let installedProbe: MemoryProbe | null = null

/**
 * Create a probe and drive it from the render-perf snapshot tick. The sampler is
 * only invoked while the HUD is enabled, so an installed-but-off probe reads
 * nothing. The returned handle unregisters it (HUD unmount).
 */
export function installMemoryProbe(sources: MemoryProbeSources): MemoryProbeHandle {
  const probe = createMemoryProbe(sources)
  installedProbe = probe
  setRenderPerfMemorySampler(nowMs => probe.sample(nowMs))
  return {
    dispose() {
      // A newer install already replaced us — leave its sampler alone.
      if (installedProbe !== probe) return
      installedProbe = null
      setRenderPerfMemorySampler(null)
    }
  }
}
