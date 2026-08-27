<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { ChevronDown, ChevronRight } from 'lucide-vue-next'

const SLIDE_MS = 500

const props = defineProps<{
  summaryLine: string
  liveLine?: string | null
  liveKey?: string | null
  expanded: boolean
  durationLabel?: string
  failed?: boolean
  /** Keep the last current-task line until the next tool starts (or the run ends). */
  forceLiveSlot?: boolean
  indentLive?: boolean
  ariaLabel?: string
  /** Hide the chevron when there is nothing to expand. */
  showChevron?: boolean
  /** Subtle pulse on the current-task line while a tool (or thinking) is live. */
  liveBusy?: boolean
}>()

const emit = defineEmits<{
  toggle: []
}>()

const restingKey = ref<string | null>(null)
const restingLine = ref('')
const departingLine = ref<string | null>(null)
const incomingLine = ref<string | null>(null)
const pushing = ref(false)
let slideTimer: ReturnType<typeof setTimeout> | null = null

function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined'
    && window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function stopPush() {
  if (slideTimer != null) {
    clearTimeout(slideTimer)
    slideTimer = null
  }
  departingLine.value = null
  incomingLine.value = null
  pushing.value = false
}

function startPush(fromLine: string, toLine: string | null) {
  if (prefersReducedMotion() || !fromLine.trim()) {
    stopPush()
    return
  }
  stopPush()
  departingLine.value = fromLine
  incomingLine.value = toLine
  pushing.value = true
  slideTimer = setTimeout(() => {
    departingLine.value = null
    incomingLine.value = null
    pushing.value = false
    slideTimer = null
  }, SLIDE_MS)
}

watch(
  () => [props.liveKey, props.liveLine, props.expanded, props.forceLiveSlot] as const,
  () => {
    if (props.expanded) return
    const key = props.liveKey ?? null
    const trimmed = (props.liveLine ?? '').trim()
    const force = props.forceLiveSlot === true

    // Push when the keyed current-task changes (tool ↔ 思考中).
    // Same key (thinking dots) updates the line in place.
    if (key && trimmed) {
      if (restingLine.value && restingKey.value !== key) {
        startPush(restingLine.value, trimmed)
      }
      restingKey.value = key
      restingLine.value = trimmed
      return
    }

    if (force) {
      if (!restingLine.value && trimmed) {
        restingKey.value = key
        restingLine.value = trimmed
      }
      return
    }

    if (restingLine.value) {
      startPush(restingLine.value, null)
      restingKey.value = null
      restingLine.value = ''
    }
  },
  { immediate: true }
)

onUnmounted(() => {
  if (slideTimer != null) window.clearTimeout(slideTimer)
})

const showLiveSlot = computed(() => {
  if (props.expanded) return false
  return pushing.value || !!restingLine.value
})

const showSummaryRow = computed(
  () => !!(props.summaryLine.trim() || props.durationLabel)
)

const showChevronIcon = computed(() => props.showChevron !== false)

const summaryToneClass = computed(() =>
  props.failed
    ? 'text-danger'
    : 'text-muted group-hover:text-foreground'
)
</script>

<template>
  <button
    type="button"
    class="tool-call-trigger group flex flex-col items-start gap-0.5 w-full min-w-0 max-w-full text-left hover:bg-hover/50 rounded-md py-0.5 transition cursor-pointer"
    :aria-expanded="expanded"
    :aria-label="ariaLabel || summaryLine"
    :title="summaryLine"
    @click="emit('toggle')"
  >
    <span
      v-if="showSummaryRow"
      class="flex items-center gap-1 min-w-0 w-full h-5"
    >
      <slot name="icon" />
      <span
        class="min-w-0 text-[13px] truncate"
        :class="summaryToneClass"
      >{{ summaryLine }}</span>
      <span
        v-if="durationLabel"
        class="shrink-0 text-[10px] text-muted/45 tabular-nums"
      >{{ durationLabel }}</span>
      <component
        v-if="showChevronIcon"
        :is="expanded ? ChevronDown : ChevronRight"
        class="tool-call-chevron h-3 w-3 shrink-0 text-muted hidden"
        aria-hidden="true"
      />
    </span>
    <span
      v-if="showLiveSlot"
      class="collapsed-run-live-slot"
    >
      <span
        class="collapsed-run-live-clip"
        :class="{ 'collapsed-run-live-clip-indent': indentLive }"
      >
        <template v-if="pushing">
          <span
            v-if="incomingLine"
            class="collapsed-run-live-line collapsed-run-live-incoming"
            :class="{ 'tool-live-pulse': liveBusy }"
          >{{ incomingLine }}</span>
          <span
            v-if="departingLine"
            class="collapsed-run-live-line collapsed-run-live-departing"
          >{{ departingLine }}</span>
        </template>
        <span
          v-else
          class="collapsed-run-live-line"
          :class="{ 'tool-live-pulse': liveBusy }"
        >{{ restingLine }}</span>
      </span>
    </span>
  </button>
</template>
