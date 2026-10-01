<script setup lang="ts">
import { computed, onScopeDispose } from 'vue'
import {
  PERF_ACTIVITY_IDLE,
  PERF_CALL_KEYS,
  PERF_GAUGE_FROZEN_TURNS,
  PERF_GAUGE_SCOPED_ROWS,
  PERF_GAUGE_VIRTUAL_BLANK,
  PERF_GAUGE_VIRTUAL_FIRST,
  PERF_GAUGE_VIRTUAL_LAST,
  PERF_GAUGE_VIRTUAL_MEASURED,
  PERF_GAUGE_VIRTUAL_OVERLAP,
  PERF_GAUGE_VIRTUAL_PULL_SPACER,
  PERF_GAUGE_VIRTUAL_SCROLL_HEIGHT,
  PERF_GAUGE_VIRTUAL_SLACK,
  PERF_GAUGE_VIRTUAL_TOTAL,
  PERF_GAUGE_VISIBLE_ROWS,
  PERF_HUD_SHORTCUT_LABEL,
  PERF_MOUNT_KEYS,
  PERF_MS_KEYS,
  PERF_MS_SCROLL_PASS,
  PERF_RENDER_KEYS,
  useRenderPerfSnapshot,
  type RenderPerfCostPer1000Px
} from '../../lib/renderPerf'
import {
  formatBytes,
  formatCount,
  formatDelta,
  installMemoryProbe,
  readDomNodeCount,
  readJsHeap,
  type MemoryGaugeReading,
  type MemoryStats
} from '../../lib/memoryProbe'

/**
 * Dev-only render performance overlay (see `lib/renderPerf.ts`).
 *
 * Mounted behind `v-if` in `ChatView.vue`, so nothing here exists while the HUD
 * is off. `pointer-events-none` + `fixed` keeps it out of the chat's way and
 * out of layout.
 *
 * Rendered as a single pre-formatted text block: the 4 Hz refresh then costs
 * one text update instead of a vnode tree.
 *
 * The frontend memory rows come from `lib/memoryProbe.ts`. The probe is installed
 * here — so it exists only while the HUD is mounted — and samples at most once a
 * second off the existing snapshot tick (no second timer).
 */
const props = defineProps<{
  /** Cheap app-side counters (transcript length, cache entries, …), read at sample time only. */
  readGauges?: () => MemoryGaugeReading[]
}>()

const memoryProbe = installMemoryProbe({
  readDomNodes: readDomNodeCount,
  readHeap: readJsHeap,
  readGauges: () => props.readGauges?.() ?? []
})
onScopeDispose(() => memoryProbe.dispose())

const snapshot = useRenderPerfSnapshot()

function pad(value: number, width: number, digits = 1): string {
  return value.toFixed(digits).padStart(width, ' ')
}

function group(
  keys: readonly string[],
  values: Record<string, number>,
  peaks: Record<string, number>,
  digits = 0,
  width = 6
): string[] {
  return keys.map(key =>
    `${key.padEnd(30)}${pad(values[key] ?? 0, width, digits)}`
      + `  pk ${pad(peaks[key] ?? 0, width, digits)}`)
}

/**
 * Current / peak / delta-since-open rows, with `n/a` for anything the webview or
 * the app cannot report (WKWebView has no `performance.memory`).
 */
function memoryLines(stats: MemoryStats | null): string[] {
  if (!stats) return ['mem  heap n/a  nodes n/a']
  return [
    `mem  heap ${formatBytes(stats.heapUsed.current)} / ${formatBytes(stats.heapTotalBytes)}`
      + `  pk ${formatBytes(stats.heapUsed.peak)}`
      + `  Δ ${formatDelta(stats.heapUsed.delta, formatBytes)}`,
    `mem  nodes ${formatCount(stats.domNodes.current)}`
      + `  pk ${formatCount(stats.domNodes.peak)}`
      + `  Δ ${formatDelta(stats.domNodes.delta, formatCount)}`,
    ...stats.gauges.map(gauge =>
      `mem  ${gauge.label} ${formatCount(gauge.metric.current)}`
        + `  pk ${formatCount(gauge.metric.peak)}`
        + `  Δ ${formatDelta(gauge.metric.delta, formatCount)}`)
  ]
}

/** `1234 px`, `12.3k px` — the normalising denominator, kept short. */
function formatDistance(px: number): string {
  if (!Number.isFinite(px) || px <= 0) return '0 px'
  if (px < 10_000) return `${Math.round(px)} px`
  return `${(px / 1000).toFixed(1)}k px`
}

/** `+155 px` / `-12 px` — direction matters for the blank-region gap. */
function formatSignedDistance(px: number): string {
  if (!Number.isFinite(px) || px === 0) return '0 px'
  const body = formatDistance(Math.abs(px))
  return px > 0 ? `+${body}` : `-${body}`
}

/** Compact per-1000-px cost: `12.3`, `0.80`, `0` — readable at 10px wide. */
function formatCost(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0'
  if (value >= 1000) return String(Math.round(value))
  if (value >= 1) return value.toFixed(1)
  return value.toFixed(2)
}

/**
 * Accumulated scroll distance and the session costs it normalises. Two rows: the
 * first is the denominator, the second reads renders, mounts, `measureElement`
 * and the scroll pass itself per 1000 px scrolled, so two runs with different
 * gestures can be compared. `n/a` until the window has moved at all.
 */
function scrollCostLines(distancePx: number, cost: RenderPerfCostPer1000Px): string[] {
  if (!(distancePx > 0)) {
    return ['scroll 0 px', 'per 1k px  n/a (nothing scrolled yet)']
  }
  return [
    `scroll ${formatDistance(distancePx)}`,
    `per 1k px  renders ${formatCost(cost.renders)}  mounts ${formatCost(cost.mounts)}`
      + `  measure ${formatCost(cost.measureElementMs)}ms`
      + `  pass ${formatCost(cost.scrollPassMs)}ms`
  ]
}

/** `worst gap    753ms @ layoutRebuild` — a number plus the operation it is blamed on. */
function activityLine(label: string, value: string, activity: string): string {
  return `${label.padEnd(10)}${value.padStart(8)} @ ${activity || PERF_ACTIVITY_IDLE}`
}

/**
 * Virtualizer geometry, one line per concern (see `publishVirtualGeometryGauges`
 * and `publishVirtualRowSlackGauges`):
 *
 * - the box: content height against the scroller's own height. `v.blank` is their
 *   difference, which *agrees by construction* (the container height is written
 *   from `v.total`) and so is marked with `*`; `v.pull` is the pull-to-load spacer
 *   that shares the scroll box and is what a stuck `v.blank` is made of.
 * - the rows: `v.slack` / `v.overlap` sum the per-row assumed-vs-actual deviation,
 *   which is where a blank stripe between rows actually shows; `v.range` and
 *   `v.measured` say how much of the window is measured rather than estimated.
 */
function virtualGeometryLines(gauges: Record<string, number>): string[] {
  const total = gauges[PERF_GAUGE_VIRTUAL_TOTAL] ?? 0
  const scrollHeight = gauges[PERF_GAUGE_VIRTUAL_SCROLL_HEIGHT] ?? 0
  const blank = gauges[PERF_GAUGE_VIRTUAL_BLANK] ?? 0
  const pullSpacer = gauges[PERF_GAUGE_VIRTUAL_PULL_SPACER] ?? 0
  const slack = gauges[PERF_GAUGE_VIRTUAL_SLACK] ?? 0
  const overlap = gauges[PERF_GAUGE_VIRTUAL_OVERLAP] ?? 0
  const first = gauges[PERF_GAUGE_VIRTUAL_FIRST] ?? -1
  const last = gauges[PERF_GAUGE_VIRTUAL_LAST] ?? -1
  const measured = gauges[PERF_GAUGE_VIRTUAL_MEASURED] ?? 0
  return [
    `v.total ${formatDistance(total).padStart(8)}`
      + `  v.scrollH ${formatDistance(scrollHeight).padStart(8)}`
      + `  v.blank* ${formatSignedDistance(blank).padStart(8)}`
      + `  v.pull ${formatDistance(pullSpacer).padStart(7)}`,
    `v.slack ${formatDistance(slack).padStart(8)}`
      + `  v.overlap ${formatDistance(overlap).padStart(7)}`
      + `  v.range ${(first < 0 ? 'none' : `${first}-${last}`).padStart(8)}`
      + `  v.measured ${String(measured).padStart(5)}`,
    '  * v.blank agrees by construction (container height := v.total);'
      + ' v.slack / v.overlap are the per-row truth'
  ]
}

const text = computed(() => {
  const s = snapshot.value
  return [
    `render perf  ${PERF_HUD_SHORTCUT_LABEL}`,
    `fps ${pad(s.fps, 6)}  gap ${pad(s.frameGapMs, 7)}ms  max ${pad(s.frameGapMaxMs, 8)}ms`
      + `  sus ${String(s.frameSuspensions).padStart(3)}`,
    // Attribution rows: the two numbers that survive a screenshot (`max` above
    // and the `ms:scrollPass` peak below), each blamed on the operation that was
    // running — `@ idle` when nothing was marked.
    activityLine('worst gap', `${Math.round(s.frameGapMaxMs)}ms`, s.frameGapMaxActivity),
    activityLine(
      'pass pk',
      `${(s.peakMsPerSecond[PERF_MS_SCROLL_PASS] ?? 0).toFixed(1)}ms`,
      s.peakScrollPassActivity
    ),
    `renders/s ${String(s.totalRendersPerSecond).padStart(4)}`
      + ` pk ${String(s.totalRendersPerSecondPeak).padStart(4)}  `
      + `vis rows ${String(s.gauges[PERF_GAUGE_VISIBLE_ROWS] ?? 0).padStart(3)}  `
      + `scoped ${String(s.gauges[PERF_GAUGE_SCOPED_ROWS] ?? 0).padStart(3)}  `
      + `frozen ${String(s.gauges[PERF_GAUGE_FROZEN_TURNS] ?? 0).padStart(3)}  `
      + `renders/row ${s.rendersPerVisibleRow.toFixed(2)}`
      + ` pk ${s.rendersPerVisibleRowPeak.toFixed(2)}`,
    ...virtualGeometryLines(s.gauges),
    ...scrollCostLines(s.scrollDistancePx, s.per1000Px),
    ...memoryLines(s.memory),
    ...group(PERF_RENDER_KEYS, s.renders, s.peakRenders),
    ...group(PERF_MOUNT_KEYS, s.mounts, s.peakMounts),
    ...group(PERF_CALL_KEYS, s.calls, s.peakCalls),
    ...group(PERF_MS_KEYS, s.msPerSecond, s.peakMsPerSecond, 1)
  ].join('\n')
})
</script>

<template>
  <div
    class="pointer-events-none fixed bottom-2 left-2 z-[250] select-none whitespace-pre rounded-md border border-white/10 bg-black/80 px-2.5 py-1.5 font-mono text-[10px] leading-[1.4] text-emerald-100/90 shadow-lg"
    aria-hidden="true"
    data-testid="render-perf-hud"
  >{{ text }}</div>
</template>
