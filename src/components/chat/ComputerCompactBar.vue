<script setup lang="ts">
import { Monitor, Maximize2, Square } from 'lucide-vue-next'
import { useWindowChrome } from '../../composables/useWindowChrome'
import { isTauriRuntime } from '../../lib/runtime'

defineProps<{
  planLine: string | null
  statusLine: string
  twoLines: boolean
}>()

const emit = defineEmits<{
  (e: 'expand'): void
  (e: 'stop'): void
}>()

const { enabled: chromeEnabled, startDrag } = useWindowChrome()

function onBarMouseDown(e: MouseEvent) {
  if (!chromeEnabled || !isTauriRuntime()) return
  if (e.button !== 0) return
  const target = e.target as HTMLElement | null
  if (target?.closest('button, [data-tauri-drag-region="false"]')) return
  void startDrag()
}
</script>

<template>
  <div
    class="computer-compact-bar h-full w-full flex items-stretch select-none overflow-hidden rounded-[14px] shadow-[0_8px_28px_hsl(var(--foreground)/0.18)]"
    :class="twoLines ? 'min-h-[62px]' : 'min-h-[48px]'"
    data-tauri-drag-region
    @mousedown="onBarMouseDown"
  >
    <div
      class="flex flex-1 min-w-0 items-center gap-2.5 pl-3 pr-2 py-2"
      data-tauri-drag-region
    >
      <div
        class="relative flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-accent-muted/70 text-accent"
        aria-hidden="true"
      >
        <span class="absolute inset-0 rounded-xl bg-accent/10 animate-pulse" />
        <Monitor class="relative w-4 h-4" />
      </div>
      <div class="flex-1 min-w-0 flex flex-col justify-center gap-0.5">
        <p
          v-if="planLine"
          class="text-[11px] leading-snug text-muted truncate"
          :title="planLine"
        >
          {{ planLine }}
        </p>
        <p
          class="text-[13px] leading-snug text-foreground truncate"
          :class="planLine ? 'font-normal' : 'font-medium'"
          :title="statusLine"
          role="status"
          aria-live="polite"
        >
          {{ statusLine }}
        </p>
      </div>
    </div>

    <div
      class="flex items-center gap-1 px-2 shrink-0 border-l border-border/40"
      data-tauri-drag-region="false"
    >
      <div class="flex items-center gap-0.5 rounded-xl bg-hover/60 p-0.5">
        <button
          type="button"
          class="h-8 w-8 rounded-[10px] flex items-center justify-center text-danger hover:bg-danger/15 transition cursor-pointer"
          title="终止"
          aria-label="终止"
          @click="emit('stop')"
        >
          <Square class="w-3.5 h-3.5" />
        </button>
        <button
          type="button"
          class="h-8 w-8 rounded-[10px] flex items-center justify-center text-muted hover:text-foreground hover:bg-card transition cursor-pointer"
          title="展开"
          aria-label="展开"
          @click="emit('expand')"
        >
          <Maximize2 class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.computer-compact-bar {
  background-color: hsl(var(--card));
}
</style>
