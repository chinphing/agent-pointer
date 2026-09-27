<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import { trackFullscreenExit } from '../../../lib/fullscreenTrack'


const { t } = useI18n()
const props = defineProps<{
  src: string
  alt?: string
}>()

const open = ref(false)
const scale = ref(1)
const translateX = ref(0)
const translateY = ref(0)
const isDragging = ref(false)
const dragStartX = ref(0)
const dragStartY = ref(0)
const dragOriginX = ref(0)
const dragOriginY = ref(0)
let unlistenFullscreen: (() => void) | undefined

function openPreview() {
  open.value = true
  scale.value = 1
  translateX.value = 0
  translateY.value = 0
  // On desktop, Esc while the OS-level window is fullscreen is consumed by the
  // system to exit fullscreen (no DOM keydown) — close when the window leaves
  // fullscreen so the user's Esc intent still dismisses the preview.
  void trackFullscreenExit(closePreview).then(fn => {
    unlistenFullscreen = fn
  })
}

function closePreview() {
  open.value = false
  unlistenFullscreen?.()
  unlistenFullscreen = undefined
}

function resetZoom() {
  scale.value = 1
  translateX.value = 0
  translateY.value = 0
}

function onWheel(e: WheelEvent) {
  e.preventDefault()
  const delta = e.deltaY > 0 ? 0.9 : 1.1
  const newScale = Math.min(Math.max(scale.value * delta, 0.25), 10)
  // zoom toward cursor position
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  const cx = (e.clientX - rect.left) / rect.width
  const cy = (e.clientY - rect.top) / rect.height
  const imgW = rect.width
  const imgH = rect.height
  translateX.value = (e.clientX - rect.left - imgW / 2) - ((e.clientX - rect.left - imgW / 2) - translateX.value) * (newScale / scale.value)
  translateY.value = (e.clientY - rect.top - imgH / 2) - ((e.clientY - rect.top - imgH / 2) - translateY.value) * (newScale / scale.value)
  scale.value = newScale
}

function onMouseDown(e: MouseEvent) {
  if (scale.value <= 1) return
  isDragging.value = true
  dragStartX.value = e.clientX
  dragStartY.value = e.clientY
  dragOriginX.value = translateX.value
  dragOriginY.value = translateY.value
}

function onMouseMove(e: MouseEvent) {
  if (!isDragging.value) return
  translateX.value = dragOriginX.value + (e.clientX - dragStartX.value)
  translateY.value = dragOriginY.value + (e.clientY - dragStartY.value)
}

function onMouseUp() {
  isDragging.value = false
}

function handleBackdropClick(e: MouseEvent) {
  // only close when clicking the backdrop itself, not the image
  if (e.target === e.currentTarget) closePreview()
}

function onDblClick(e: MouseEvent) {
  // double-click image to reset zoom
  if (e.target !== e.currentTarget) return
  resetZoom()
}

function onKeydown(e: KeyboardEvent) {
  // Capture-phase + stopImmediatePropagation: while the preview is open, Esc
  // must not leak to other document/window listeners (DiffView maximized,
  // TerminalLiveOutputModal fullscreen, other modals…).
  if (e.key === 'Escape' && open.value) {
    e.stopImmediatePropagation()
    closePreview()
  }
  if (e.key === 'r' && open.value) {
    e.stopImmediatePropagation()
    resetZoom()
  }
}

onMounted(() => document.addEventListener('keydown', onKeydown, true))
onBeforeUnmount(() => document.removeEventListener('keydown', onKeydown, true))
</script>

<template>
  <!-- thumbnail -->
  <img
    :src="src"
    :alt="alt"
    class="max-h-64 max-w-full rounded-xl border border-border object-contain cursor-zoom-in"
    @click="openPreview"
  />

  <!-- full-screen overlay -->
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[9999] flex items-center justify-center bg-black/70 backdrop-blur-sm overflow-hidden select-none"
      @wheel="onWheel"
      @mousedown="onMouseDown"
      @mousemove="onMouseMove"
      @mouseup="onMouseUp"
      @mouseleave="onMouseUp"
      @click="handleBackdropClick"
    >
      <div
        class="relative"
        :style="{
          transform: `translate(${translateX}px, ${translateY}px) scale(${scale})`,
          cursor: scale > 1 ? (isDragging ? 'grabbing' : 'grab') : 'grab',
        }"
        @click.stop
        @dblclick="onDblClick"
      >
        <img
          :src="src"
          :alt="alt"
          class="max-h-[90vh] max-w-[90vw] object-contain rounded-lg shadow-2xl"
          draggable="false"
        />
      </div>

      <!-- zoom indicator -->
      <div
        class="fixed bottom-6 left-1/2 -translate-x-1/2 px-3 py-1 rounded-full bg-black/50 text-white/80 text-xs pointer-events-none"
      >
        {{ Math.round(scale * 100) }}%
      </div>

      <!-- reset hint -->
      <div
        v-if="scale !== 1"
        class="fixed top-4 right-4 px-3 py-1.5 rounded-lg bg-white/10 text-white/60 text-xs pointer-events-none"
      >
        {{ t('chat.message.zoomHint') }}
      </div>
    </div>
  </Teleport>
</template>
