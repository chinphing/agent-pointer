<script setup lang="ts">
import { computed, onScopeDispose, ref } from 'vue'
import {
  PERF_ACTIVITY_IDLE,
  PERF_CALL_KEYS,
  PERF_GAUGE_FROZEN_TURNS,
  PERF_GAUGE_SCOPED_ROWS,
  PERF_GAUGE_VIRTUAL_BLANK,
  PERF_GAUGE_VIRTUAL_ESTIMATE,
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
import { writeClipboardText, CLIPBOARD_FEEDBACK_MS } from '../../lib/clipboardText'

/**
 * Dev-only render performance overlay (see `lib/renderPerf.ts`).
 *
 * Mounted behind `v-if` in `ChatView.vue`, so nothing here exists while the HUD
 * is off — no timer, no click target, no listener.
 *
 * Rendered as a single pre-formatted text block: the 4 Hz refresh then costs
 * one text update instead of a vnode tree.
 *
 * Clicking the block copies that block (see `lib/clipboardText.ts`), so the
 * numbers can be pasted instead of screenshotted, and the title row reports the
 * outcome while it is on screen. It is a debug surface, so it takes the clicks in
 * its own box rather than passing them through — `pointer-events-none` is
 * deliberately gone. It stays `aria-hidden` and out of the tab order: a focusable
 * element would put this dense debug text into the accessibility tree, and the
 * overlay is decoration by design.
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

// --- click to copy ----------------------------------------------------------

/** Title row while idle: the toggle shortcut plus the click affordance. */
const TITLE_LINE = `render perf  ${PERF_HUD_SHORTCUT_LABEL}  ·  click to copy`

type CopyState = 'idle' | 'copied' | 'failed'

const copyState = ref<CopyState>('idle')
/**
 * The confirmation's timer. Only ever created by a click (there is no timer while
 * the overlay is idle, and none at all while it is unmounted), and cleared with
 * the component so a stray timer cannot outlive the HUD.
 */
let copyResetTimer: ReturnType<typeof setTimeout> | null = null

function clearCopyResetTimer(): void {
  if (copyResetTimer === null) return
  clearTimeout(copyResetTimer)
  copyResetTimer = null
}

/**
 * Copy what the overlay is showing. Reads `text` — the same computed the rows are
 * built from — so the clipboard gets the exact rendered string instead of a
 * re-render, and reports the outcome in the title row either way.
 */
async function copyHudText(): Promise<void> {
  const copied = await writeClipboardText(text.value)
  copyState.value = copied ? 'copied' : 'failed'
  clearCopyResetTimer()
  copyResetTimer = setTimeout(() => {
    copyResetTimer = null
    copyState.value = 'idle'
  }, CLIPBOARD_FEEDBACK_MS)
}

onScopeDispose(() => {
  clearCopyResetTimer()
})

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
 *   which is where a blank stripe between rows actually shows; each carries its
 *   session `pk`, because the stripe that matters is a transient one that has
 *   already passed by the time the HUD is read. `v.range` and `v.measured` say how
 *   much of the window is measured rather than estimated, and `v.est` is the height
 *   currently assumed for the rows that are *not* — the number placing them.
 */
function virtualGeometryLines(
  gauges: Record<string, number>,
  peaks: Record<string, number>
): string[] {
  const total = gauges[PERF_GAUGE_VIRTUAL_TOTAL] ?? 0
  const scrollHeight = gauges[PERF_GAUGE_VIRTUAL_SCROLL_HEIGHT] ?? 0
  const blank = gauges[PERF_GAUGE_VIRTUAL_BLANK] ?? 0
  const pullSpacer = gauges[PERF_GAUGE_VIRTUAL_PULL_SPACER] ?? 0
  const slack = gauges[PERF_GAUGE_VIRTUAL_SLACK] ?? 0
  const overlap = gauges[PERF_GAUGE_VIRTUAL_OVERLAP] ?? 0
  const first = gauges[PERF_GAUGE_VIRTUAL_FIRST] ?? -1
  const last = gauges[PERF_GAUGE_VIRTUAL_LAST] ?? -1
  const measured = gauges[PERF_GAUGE_VIRTUAL_MEASURED] ?? 0
  const estimate = gauges[PERF_GAUGE_VIRTUAL_ESTIMATE] ?? 0
  return [
    `v.total ${formatDistance(total).padStart(8)}`
      + `  v.scrollH ${formatDistance(scrollHeight).padStart(8)}`
      + `  v.blank* ${formatSignedDistance(blank).padStart(8)}`
      + `  v.pull ${formatDistance(pullSpacer).padStart(7)}`,
    `v.slack ${formatDistance(slack).padStart(8)}`
      + `  pk ${formatDistance(peaks[PERF_GAUGE_VIRTUAL_SLACK] ?? 0).padStart(8)}`
      + `  v.overlap ${formatDistance(overlap).padStart(7)}`
      + `  pk ${formatDistance(peaks[PERF_GAUGE_VIRTUAL_OVERLAP] ?? 0).padStart(7)}`,
    `v.range ${(first < 0 ? 'none' : `${first}-${last}`).padStart(8)}`
      + `  v.measured ${String(measured).padStart(5)}`
      + `  v.est ${formatDistance(estimate).padStart(7)}`,
    '  * v.blank agrees by construction (container height := v.total);'
      + ' v.slack / v.overlap are the per-row truth, pk is their session peak'
  ]
}

/**
 * Body rows, without the title line — the overlay's own content, in order.
 */
const bodyLines = computed<string[]>(() => {
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
    ...virtualGeometryLines(s.gauges, s.peakGauges),
    ...scrollCostLines(s.scrollDistancePx, s.per1000Px),
    ...memoryLines(s.memory),
    ...group(PERF_RENDER_KEYS, s.renders, s.peakRenders),
    ...group(PERF_MOUNT_KEYS, s.mounts, s.peakMounts),
    ...group(PERF_CALL_KEYS, s.calls, s.peakCalls),
    ...group(PERF_MS_KEYS, s.msPerSecond, s.peakMsPerSecond, 1)
  ]
})

/**
 * The exact text the overlay copies: the title row plus the body rows, from the
 * same computed the template renders — never a re-render, and never carrying the
 * copy confirmation.
 */
const text = computed(() => [TITLE_LINE, ...bodyLines.value].join('\n'))

/**
 * What the template renders: identical to `text` except while a copy result is
 * showing, when the title row is swapped for the marker.
 */
const displayText = computed(() => {
  if (copyState.value === 'idle') return text.value
  const marker = copyState.value === 'copied' ? 'copied' : 'copy failed'
  return [`render perf  ${marker}`, ...bodyLines.value].join('\n')
})
</script>

<template>
  <div
    class="fixed bottom-2 left-2 z-[250] cursor-pointer select-none whitespace-pre rounded-md border border-white/10 bg-black/80 px-2.5 py-1.5 font-mono text-[10px] leading-[1.4] text-emerald-100/90 shadow-lg"
    aria-hidden="true"
    data-testid="render-perf-hud"
    title="Click to copy the HUD text"
    @click="copyHudText"
  >{{ displayText }}</div>
</template>
