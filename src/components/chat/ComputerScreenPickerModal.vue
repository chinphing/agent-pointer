<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Monitor, X } from 'lucide-vue-next'
import type { ComputerMonitor } from '../../types/chat'
import {
  layoutComputerMonitors,
  monitorShortLabel
} from '../../lib/computerMonitorLayout'


const { t } = useI18n()
const open = defineModel<boolean>('open', { required: true })

const props = defineProps<{
  monitors: ComputerMonitor[]
  loading: boolean
  error: string | null
}>()

const emit = defineEmits<{
  (e: 'pick', monitorId: string): void
}>()

const hoveredId = ref<string | null>(null)

const layout = computed(() => layoutComputerMonitors(props.monitors ?? []))

function close() {
  open.value = false
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

function pick(id: string) {
  emit('pick', id)
}

watch(
  open,
  v => {
    if (v) document.addEventListener('keydown', onKeydown)
    else document.removeEventListener('keydown', onKeydown)
  },
  { immediate: true }
)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[220] flex items-center justify-center bg-foreground/32 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      :aria-label="t('computer.pickScreen.ariaLabel')"
      @click.self="close"
    >
      <div class="relative w-full max-w-xl rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl overflow-hidden">
        <header class="flex items-start gap-3 px-5 pt-5 pb-3 border-b border-border">
          <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
            <Monitor class="w-4 h-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1 pr-8">
            <h2 class="text-base font-semibold text-foreground">{{ t('computer.pickScreen.title') }}</h2>
            <p class="text-[12px] text-muted mt-0.5 leading-relaxed">
              {{ t('computer.pickScreen.description') }}
            </p>
          </div>
          <button
            type="button"
            class="absolute top-4 right-4 p-2 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors"
            :title="t('computer.pickScreen.closeTitle')"
            @click="close"
          >
            <X class="w-4 h-4" />
          </button>
        </header>

        <div class="p-5">
          <div v-if="error" class="rounded-xl border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">
            {{ error }}
          </div>
          <div v-else-if="loading" class="text-sm text-muted py-2">{{ t('computer.pickScreen.loading') }}</div>
          <div v-else-if="!layout.rects.length" class="text-center text-sm text-muted py-6">
            {{ t('computer.pickScreen.noScreens') }}
          </div>
          <div v-else class="space-y-4">
            <div
              class="relative mx-auto rounded-xl border border-dashed border-border/80 bg-[hsl(var(--card))]/60"
              :style="{ width: `${layout.canvasWidth}px`, height: `${layout.canvasHeight}px`, maxWidth: '100%' }"
            >
              <button
                v-for="rect in layout.rects"
                :key="rect.id"
                type="button"
                class="absolute rounded-lg border-2 transition-all cursor-pointer flex flex-col items-center justify-center gap-0.5 px-1 text-center overflow-hidden"
                :class="hoveredId === rect.id
                  ? 'border-foreground/40 bg-hover'
                  : 'border-border bg-card/90 hover:border-border hover:bg-hover'"
                :style="{
                  left: `${rect.x}px`,
                  top: `${rect.y}px`,
                  width: `${rect.width}px`,
                  height: `${rect.height}px`
                }"
                @mouseenter="hoveredId = rect.id"
                @mouseleave="hoveredId = null"
                @click="pick(rect.id)"
              >
                <span class="text-[11px] font-medium text-foreground truncate max-w-full">
                  {{ monitorShortLabel(rect.monitor) }}
                </span>
                <span class="text-[10px] text-muted truncate max-w-full">
                  {{ rect.monitor.width }}×{{ rect.monitor.height }}
                </span>
              </button>
            </div>
            <p class="text-[11px] text-muted text-center">
              {{ t('computer.pickScreen.layoutHint') }}
            </p>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
