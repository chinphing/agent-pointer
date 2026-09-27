<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

import { ListChecks, Monitor, Maximize2, Square } from 'lucide-vue-next'
import WindowDragRegion from '../layout/WindowDragRegion.vue'
import type { TaskBoardCompactSummary } from '../../lib/taskBoardCollapsedLine'

defineProps<{
  planSummary: TaskBoardCompactSummary | null
  planLine: string | null
  statusLine: string
  twoLines: boolean
}>()

const emit = defineEmits<{
  (e: 'expand'): void
  (e: 'stop'): void
}>()
</script>

<template>
  <WindowDragRegion
    region="compact-bar-shell"
    class="computer-compact-bar h-full w-full flex items-stretch select-none overflow-hidden rounded-[14px] shadow-[0_8px_28px_hsl(var(--foreground)/0.18)]"
    :class="twoLines ? 'min-h-[62px]' : 'min-h-[48px]'"
  >
    <div class="flex flex-1 min-w-0 items-center gap-2.5 pl-3 pr-2 py-2">
      <div
        class="relative flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-accent-muted/70 text-accent"
        aria-hidden="true"
      >
        <span class="absolute inset-0 rounded-xl bg-accent/10 animate-pulse" />
        <Monitor class="relative w-4 h-4" />
      </div>
      <WindowDragRegion region="compact-bar-status" class="flex-1 min-w-0 flex flex-col justify-center gap-0.5">
        <div
          v-if="planSummary"
          class="flex min-w-0 items-center gap-2"
          :title="planLine ?? undefined"
        >
          <ListChecks class="w-3.5 h-3.5 shrink-0 text-accent" aria-hidden="true" />
          <span class="text-[11px] font-medium leading-[1.25rem] tabular-nums text-muted shrink-0">
            {{ planSummary.progress }}
          </span>
          <p class="min-w-0 flex-1 truncate text-[11px] leading-[1.25rem] text-muted">
            {{ planSummary.taskLine }}
          </p>
        </div>
        <p
          class="text-[13px] leading-[1.25rem] text-foreground truncate"
          :class="planSummary ? 'font-normal' : 'font-medium'"
          :title="statusLine"
          role="status"
          aria-live="polite"
        >
          {{ statusLine }}
        </p>
      </WindowDragRegion>
    </div>

    <WindowDragRegion
      region="compact-bar-actions"
      class="flex items-center gap-1 px-2 shrink-0 border-l border-border/40"
    >
      <div class="flex items-center gap-0.5 rounded-xl bg-hover/60 p-0.5">
        <button
          type="button"
          class="h-8 w-8 rounded-[10px] flex items-center justify-center text-danger hover:bg-danger/15 transition cursor-pointer"
          :title="t('chat.s_ff6c6a')"
          :aria-label="t('chat.s_ff6c6a')"
          @click="emit('stop')"
        >
          <Square class="w-3.5 h-3.5" />
        </button>
        <button
          type="button"
          class="h-8 w-8 rounded-[10px] flex items-center justify-center text-muted hover:text-foreground hover:bg-card transition cursor-pointer"
          :title="t('workspace.expand')"
          :aria-label="t('workspace.expand')"
          @click="emit('expand')"
        >
          <Maximize2 class="w-3.5 h-3.5" />
        </button>
      </div>
    </WindowDragRegion>
  </WindowDragRegion>
</template>

<style scoped>
.computer-compact-bar {
  background-color: hsl(var(--card));
}
</style>
