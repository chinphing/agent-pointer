<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

import { computed, ref, watch } from 'vue'

const props = defineProps<{
  xmlThoughts?: string
  thoughtsDebugEnabled?: boolean
  isStreaming?: boolean
}>()

const thoughtsBoxRef = ref<HTMLElement | null>(null)
const thoughtsOpen = ref(false)
/** User scrolled the thoughts box away from the bottom — stop auto-following until reset. */
const thoughtsUserScrolled = ref(false)
let thoughtsScrollFrame: number | null = null

const showXmlThoughts = computed(() => {
  const text = props.xmlThoughts?.trim()
  if (!text) return false
  if (props.isStreaming) return true
  return props.thoughtsDebugEnabled === true
})

/** Streaming: ~3-line scroll preview. Debug after stream: collapsed until expanded. */
const showThoughtsBody = computed(() => props.isStreaming || thoughtsOpen.value)

const thoughtsCompactPreview = computed(
  () => props.isStreaming === true && !thoughtsOpen.value
)

watch(
  () => props.isStreaming,
  (streaming, prev) => {
    if (streaming) {
      thoughtsOpen.value = false
      thoughtsUserScrolled.value = false
      return
    }
    if (prev) thoughtsOpen.value = false
  },
  { immediate: true }
)

function onThoughtsBoxScroll() {
  const el = thoughtsBoxRef.value
  if (!el || !props.isStreaming) return
  // If user scrolled more than 8px away from the bottom, stop auto-following.
  const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight
  if (distanceFromBottom > 8) thoughtsUserScrolled.value = true
  else if (distanceFromBottom <= 2) thoughtsUserScrolled.value = false
}

function onThoughtsBoxWheel(event: WheelEvent) {
  if (!props.isStreaming) return
  if (event.deltaY < 0) thoughtsUserScrolled.value = true
}

watch(
  () => props.xmlThoughts,
  () => {
    if (!props.isStreaming || thoughtsOpen.value || thoughtsUserScrolled.value) return
    const el = thoughtsBoxRef.value
    if (!el) return
    if (thoughtsScrollFrame != null) return
    thoughtsScrollFrame = requestAnimationFrame(() => {
      thoughtsScrollFrame = null
      if (thoughtsUserScrolled.value) return
      el.scrollTop = el.scrollHeight
    })
  }
)

function toggleThoughts() {
  thoughtsOpen.value = !thoughtsOpen.value
}

const hasContent = computed(() => showXmlThoughts.value)
</script>

<template>
  <div v-if="hasContent" class="space-y-2">
    <div
      v-if="showXmlThoughts"
      class="overflow-hidden px-3"
      :class="isStreaming || thoughtsOpen ? 'w-full' : 'w-fit max-w-full'"
    >
      <button
        v-if="!isStreaming"
        type="button"
        class="inline-flex items-center gap-1.5 cursor-pointer select-none hover:bg-hover/60 transition rounded-md -mx-0.5 px-0.5 py-1"
        :aria-expanded="thoughtsOpen"
        @click="toggleThoughts"
      >
        <span class="shrink-0 text-[11px] text-muted font-medium">{{ t('chat.s_0178ef') }}</span>
        <span
          class="inline-block w-3 shrink-0 text-muted text-center text-[10px] transition-transform pt-0.5"
          :class="thoughtsOpen ? 'rotate-90' : ''"
        >▸</span>
      </button>
      <div
        v-if="showThoughtsBody"
        ref="thoughtsBoxRef"
        class="text-[12px] leading-relaxed text-muted whitespace-pre-wrap break-words"
        :class="
          thoughtsCompactPreview || isStreaming
            ? 'max-h-[3rem] overflow-y-auto overflow-x-hidden mt-1'
            : 'mt-2 border-t border-border/30 pt-2'
        "
        @scroll="onThoughtsBoxScroll"
        @wheel="onThoughtsBoxWheel"
      >{{ xmlThoughts }}</div>
    </div>
  </div>
</template>
