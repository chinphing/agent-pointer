<script setup lang="ts">
import { computed } from 'vue'
import {
  PERF_CALL_KEYS,
  PERF_GAUGE_SCOPED_ROWS,
  PERF_GAUGE_VISIBLE_ROWS,
  PERF_HUD_SHORTCUT_LABEL,
  PERF_MOUNT_KEYS,
  PERF_MS_KEYS,
  PERF_RENDER_KEYS,
  useRenderPerfSnapshot
} from '../../lib/renderPerf'

/**
 * Dev-only render performance overlay (see `lib/renderPerf.ts`).
 *
 * Mounted behind `v-if` in `ChatView.vue`, so nothing here exists while the HUD
 * is off. `pointer-events-none` + `fixed` keeps it out of the chat's way and
 * out of layout.
 *
 * Rendered as a single pre-formatted text block: the 4 Hz refresh then costs
 * one text update instead of a vnode tree.
 */
const snapshot = useRenderPerfSnapshot()

function pad(value: number, width: number, digits = 1): string {
  return value.toFixed(digits).padStart(width, ' ')
}

function group(keys: readonly string[], values: Record<string, number>, digits = 0, width = 6): string[] {
  return keys.map(key => `${key.padEnd(30)}${pad(values[key] ?? 0, width, digits)}`)
}

const text = computed(() => {
  const s = snapshot.value
  return [
    `render perf  ${PERF_HUD_SHORTCUT_LABEL}`,
    `fps ${pad(s.fps, 6)}  gap ${pad(s.frameGapMs, 7)}ms  max ${pad(s.frameGapMaxMs, 8)}ms`,
    `renders/s ${String(s.totalRendersPerSecond).padStart(4)}  `
      + `vis rows ${String(s.gauges[PERF_GAUGE_VISIBLE_ROWS] ?? 0).padStart(3)}  `
      + `scoped ${String(s.gauges[PERF_GAUGE_SCOPED_ROWS] ?? 0).padStart(3)}  `
      + `renders/row ${s.rendersPerVisibleRow.toFixed(2)}`,
    ...group(PERF_RENDER_KEYS, s.renders),
    ...group(PERF_MOUNT_KEYS, s.mounts),
    ...group(PERF_CALL_KEYS, s.calls),
    ...group(PERF_MS_KEYS, s.msPerSecond, 1)
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
