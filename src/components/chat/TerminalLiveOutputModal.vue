<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Maximize2, Minimize2, Terminal, X } from 'lucide-vue-next'
import { nextFollowOutputAfterScroll } from '../../lib/messageListScrollFollow'


const { t } = useI18n()
const props = defineProps<{
  command: string
  output: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const outputEl = ref<HTMLElement | null>(null)
const fullscreen = ref(false)
/** User scrolled away from the live tail — stop auto-following until they return. */
const followOutput = ref(true)
let touchStartY: number | null = null
let lastScrollTop = 0
const ATTACH_BOTTOM_PX = 8
const DETACH_BOTTOM_PX = 48

const hasOutput = computed(() => props.output.trim().length > 0)

function distanceFromBottom(): number {
  const el = outputEl.value
  if (!el) return 0
  return el.scrollHeight - el.scrollTop - el.clientHeight
}

function scrollOutputToBottom() {
  const el = outputEl.value
  if (!el) return
  followOutput.value = true
  el.scrollTop = el.scrollHeight
  lastScrollTop = el.scrollTop
}

function onOutputScroll() {
  const el = outputEl.value
  const distance = distanceFromBottom()
  const scrollingUp = el != null && el.scrollTop < lastScrollTop - 1
  followOutput.value = nextFollowOutputAfterScroll({
    followOutput: followOutput.value,
    distanceFromBottom: distance,
    scrollingUp,
    attachPx: ATTACH_BOTTOM_PX,
    detachPx: DETACH_BOTTOM_PX
  })
  if (el) lastScrollTop = el.scrollTop
}

function onOutputWheel(event: WheelEvent) {
  if (event.deltaY < 0) followOutput.value = false
}

function onOutputTouchStart(event: TouchEvent) {
  touchStartY = event.touches[0]?.clientY ?? null
}

function onOutputTouchMove(event: TouchEvent) {
  if (touchStartY == null) return
  const y = event.touches[0]?.clientY
  if (y == null) return
  if (y - touchStartY > 8) followOutput.value = false
}

function onOutputTouchEnd() {
  touchStartY = null
}

function close() {
  fullscreen.value = false
  emit('close')
}


function toggleFullscreen() {
  fullscreen.value = !fullscreen.value
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    if (fullscreen.value) fullscreen.value = false
    else close()
  }
}

watch(
  () => props.output,
  () => {
    if (!followOutput.value) return
    void nextTick(scrollOutputToBottom)
  },
  { immediate: true }
)

document.addEventListener('keydown', onKeydown)
onUnmounted(() => document.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <div
      class="fixed inset-0 z-[240]"
      :class="
        fullscreen
          ? 'bg-[hsl(var(--card-elevated))]'
          : 'flex items-center justify-center bg-foreground/32 p-4'
      "
      role="dialog"
      aria-modal="true"
      :aria-label="t('terminalLiveOutput.ariaLabel')"
      @click.self="!fullscreen && close()"
    >
      <div
        class="relative flex flex-col overflow-hidden border border-border bg-[hsl(var(--card-elevated))] shadow-2xl"
        :class="
          fullscreen
            ? 'h-full w-full rounded-none border-0 shadow-none'
            : 'h-[70vh] max-h-[70vh] w-full max-w-2xl rounded-2xl'
        "
      >
        <header
          class="flex shrink-0 items-start gap-3 border-b border-border px-5 py-4"
          :class="fullscreen ? 'pr-24' : 'pr-20'"
        >
          <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-hover">
            <Terminal class="h-4 w-4 text-muted" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <h2 class="text-base font-semibold text-foreground">{{ t('terminalLiveOutput.title') }}</h2>
              <span
                class="inline-flex h-2 w-2 shrink-0 rounded-full bg-accent"
                aria-hidden="true"
                :title="t('terminalLiveOutput.runningTitle')"
              />
            </div>
            <p class="mt-1 text-[12px] text-muted leading-relaxed">
              {{ t('terminalLiveOutput.description') }}
            </p>
          </div>
          <div class="absolute top-4 right-4 flex items-center gap-1">
            <button
              type="button"
              class="rounded-lg p-2 text-muted transition-colors hover:bg-hover cursor-pointer"
              :aria-label="fullscreen ? t('terminalLiveOutput.exitFullscreen') : t('terminalLiveOutput.fullscreen')"
              @click="toggleFullscreen"
            >
              <Minimize2 v-if="fullscreen" class="h-4 w-4" />
              <Maximize2 v-else class="h-4 w-4" />
            </button>
            <button
              type="button"
              class="rounded-lg p-2 text-muted transition-colors hover:bg-hover cursor-pointer"
              :aria-label="t('terminalLiveOutput.close')"
              @click="close"
            >
              <X class="h-4 w-4" />
            </button>
          </div>
        </header>

        <div class="flex min-h-0 flex-1 flex-col gap-3 px-5 py-4">
          <div class="shrink-0">
            <div class="mb-1 text-[10px] uppercase tracking-wider text-muted">{{ t('terminalLiveOutput.command') }}</div>
            <pre class="max-h-64 overflow-y-auto text-[12px] font-mono whitespace-pre-wrap break-all rounded-lg border border-border bg-[hsl(var(--code-bg))] p-2.5 text-foreground">{{ command || '—' }}</pre>
          </div>
          <div class="flex min-h-0 flex-1 flex-col">
            <div class="mb-1 shrink-0 text-[10px] uppercase tracking-wider text-muted">{{ t('terminalLiveOutput.consoleOutput') }}</div>
            <pre
              ref="outputEl"
              class="min-h-0 flex-1 overflow-y-auto text-[12px] font-mono whitespace-pre-wrap break-all rounded-lg border border-border bg-[hsl(var(--code-bg))] p-2.5 text-foreground"
              @scroll="onOutputScroll"
              @wheel="onOutputWheel"
              @touchstart.passive="onOutputTouchStart"
              @touchmove.passive="onOutputTouchMove"
              @touchend="onOutputTouchEnd"
              @touchcancel="onOutputTouchEnd"
            >{{ hasOutput ? output : t('terminalLiveOutput.noOutput') }}</pre>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
