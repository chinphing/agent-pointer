<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  buildCron,
  describeCron,
  parseCron,
  WEEKDAY_LABELS,
  type CronMode,
  type CronPreset
} from '../../../lib/cronSchedule'

const props = defineProps<{ modelValue: string }>()
const emit = defineEmits<{ (e: 'update:modelValue', v: string): void }>()

// Internal preset state, synced bidirectionally with the raw cron string.
// On any param change we rebuild the cron and emit; when the parent pushes a
// new raw value (e.g. loading an existing job), we re-parse it into preset.
const preset = ref<CronPreset>(parseCron(props.modelValue))

// Time-of-day shown as an HTML <input type="time"> "HH:MM" string, backed by
// the preset's hour/minute. Kept in sync via watchers below.
const timeOfDay = computed<string>({
  get: () => `${pad(preset.value.hour ?? 9)}:${pad(preset.value.minute ?? 0)}`,
  set(v: string) {
    const [h, m] = v.split(':').map(n => parseInt(n, 10))
    preset.value = {
      ...preset.value,
      hour: Number.isFinite(h) ? h : 0,
      minute: Number.isFinite(m) ? m : 0
    }
  }
})

const interval = computed<number>({
  get: () => preset.value.interval ?? (preset.value.mode === 'everyNHours' ? 2 : 5),
  set(v: number) {
    preset.value = { ...preset.value, interval: Number.isFinite(v) ? v : 1 }
  }
})

const weekday = computed<number>({
  get: () => preset.value.weekday ?? 1,
  set(v: number) {
    preset.value = { ...preset.value, weekday: v }
  }
})

const dayOfMonth = computed<number>({
  get: () => preset.value.dayOfMonth ?? 1,
  set(v: number) {
    preset.value = { ...preset.value, dayOfMonth: Number.isFinite(v) ? v : 1 }
  }
})

const rawExpr = computed<string>({
  get: () => preset.value.raw ?? props.modelValue ?? '',
  set(v: string) {
    preset.value = { ...preset.value, raw: v }
  }
})

const modeOptions: { value: CronMode; label: string }[] = [
  { value: 'everyMinute', label: '每分钟' },
  { value: 'everyNMinutes', label: '每隔 N 分钟' },
  { value: 'everyNHours', label: '每隔 N 小时' },
  { value: 'dailyAt', label: '每天定时' },
  { value: 'weeklyAt', label: '每周定时' },
  { value: 'monthlyAt', label: '每月定时' },
  { value: 'custom', label: '自定义表达式' }
]

function setMode(m: CronMode) {
  // Carry over sensible defaults when switching into a mode that lacks params.
  const next: CronPreset = { ...preset.value, mode: m }
  if (m === 'everyNMinutes' && !next.interval) next.interval = 5
  if (m === 'everyNHours' && !next.interval) next.interval = 2
  if ((m === 'dailyAt' || m === 'weeklyAt' || m === 'monthlyAt') && next.hour == null) {
    next.hour = 9
    next.minute = 0
  }
  if (m === 'monthlyAt' && next.dayOfMonth == null) next.dayOfMonth = 1
  if (m === 'weeklyAt' && next.weekday == null) next.weekday = 1
  if (m === 'custom' && !next.raw) next.raw = props.modelValue || '0 * * * * *'
  preset.value = next
}

// Emit whenever the preset changes. `custom` mode preserves the raw string
// verbatim so advanced expressions round-trip without normalization.
watch(
  preset,
  p => {
    const built = buildCron(p)
    if (built !== props.modelValue) emit('update:modelValue', built)
  },
  { deep: true }
)

// Re-parse when the parent pushes a different raw value (e.g. opening the form
// for an existing job, or resetting). Avoids clobbering user edits by only
// re-parsing when the incoming value differs from what we'd emit.
watch(
  () => props.modelValue,
  incoming => {
    if (incoming === buildCron(preset.value)) return
    preset.value = parseCron(incoming)
  }
)

function pad(n: number): string {
  return String(Math.trunc(n)).padStart(2, '0')
}
</script>

<template>
  <div class="space-y-2">
    <!-- Repeat mode + inline params on one row. flex-wrap keeps it graceful on
         narrow widths. Custom mode has no inline params (raw input below). -->
    <div class="flex items-center gap-2 flex-wrap">
      <span class="text-[11px] text-muted w-10 shrink-0">调度</span>
      <select
        :value="preset.mode"
        class="input-base w-32 shrink-0"
        @change="setMode(($event.target as HTMLSelectElement).value as CronMode)"
      >
        <option v-for="o in modeOptions" :key="o.value" :value="o.value">{{ o.label }}</option>
      </select>

      <template v-if="preset.mode === 'everyNMinutes'">
        <span class="text-[11px] text-muted">间隔</span>
        <input v-model.number="interval" type="number" min="1" max="59" class="input-base w-20" />
        <span class="text-[11px] text-muted">分钟</span>
      </template>

      <template v-else-if="preset.mode === 'everyNHours'">
        <span class="text-[11px] text-muted">间隔</span>
        <input v-model.number="interval" type="number" min="1" max="23" class="input-base w-20" />
        <span class="text-[11px] text-muted">小时</span>
      </template>

      <template v-else-if="preset.mode === 'dailyAt'">
        <input v-model="timeOfDay" type="time" class="input-base w-28" />
      </template>

      <template v-else-if="preset.mode === 'weeklyAt'">
        <select v-model.number="weekday" class="input-base w-20">
          <option v-for="(label, idx) in WEEKDAY_LABELS" :key="idx" :value="idx">{{ label }}</option>
        </select>
        <input v-model="timeOfDay" type="time" class="input-base w-28" />
      </template>
      <template v-else-if="preset.mode === 'monthlyAt'">
        <input v-model.number="dayOfMonth" type="number" min="1" max="31" class="input-base w-16" />
        <span class="text-[11px] text-muted">日</span>
        <input v-model="timeOfDay" type="time" class="input-base w-28" />
      </template>
    </div>

    <!-- Custom: raw expression on its own row. -->
    <div v-if="preset.mode === 'custom'" class="space-y-1">
      <input
        v-model="rawExpr"
        class="input-base font-mono w-full"
        placeholder="0 * * * * *  (秒 分 时 日 月 周)"
      />
      <p class="text-[11px] text-muted">6 段、秒级；字段按本地时区解释。</p>
    </div>

    <!-- Preview footer: Chinese schedule summary + raw cron (hover title). -->
    <div
      class="flex items-center gap-1.5 text-[11px] pt-2 mt-1 border-t border-border/60 min-w-0"
      :title="buildCron(preset)"
    >
      <span class="text-muted shrink-0">预览</span>
      <span class="text-foreground truncate">{{ describeCron(buildCron(preset)) }}</span>
      <span class="text-muted/50 shrink-0">·</span>
      <span class="text-muted/70 font-mono truncate">{{ buildCron(preset) }}</span>
    </div>
  </div>
</template>

<style scoped>
.input-base {
  height: 2.25rem;
  border-radius: 0.5rem;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--card));
  color: hsl(var(--foreground));
  padding: 0 0.625rem;
  font-size: 0.8125rem;
  outline: none;
}
.input-base:focus {
  border-color: hsl(var(--accent));
}
select.input-base {
  appearance: none;
  color-scheme: light dark;
  /* Custom chevron: appearance:none removes the native arrow, so render one
     ourselves via an inline SVG so the control still reads as a dropdown. */
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%23a3a3a3' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><polyline points='6 9 12 15 18 9'/></svg>");
  background-repeat: no-repeat;
  background-position: right 0.5rem center;
  background-size: 0.875rem 0.875rem;
  padding-right: 1.75rem;
}
input[type='time'].input-base {
  color-scheme: light dark;
}
</style>
