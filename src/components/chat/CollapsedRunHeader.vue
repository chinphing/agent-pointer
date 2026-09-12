<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { ChevronDown, ChevronRight } from 'lucide-vue-next'
import ToolKindIcon from './ToolKindIcon.vue'
import ToolLiveSweepText from './ToolLiveSweepText.vue'

const SLIDE_MS = 500

type LiveSnap = {
  text: string
  toolName: string | null
}

const props = defineProps<{
  summaryLine: string
  liveLine?: string | null
  liveToolName?: string | null
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
const resting = ref<LiveSnap>({ text: '', toolName: null })
const departing = ref<LiveSnap | null>(null)
const incoming = ref<LiveSnap | null>(null)
const pushing = ref(false)
let slideTimer: ReturnType<typeof setTimeout> | null = null

function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined'
    && window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function snapFromProps(): LiveSnap {
  return {
    text: (props.liveLine ?? '').trim(),
    toolName: props.liveToolName?.trim() || null
  }
}

function stopPush() {
  if (slideTimer != null) {
    clearTimeout(slideTimer)
    slideTimer = null
  }
  departing.value = null
  incoming.value = null
  pushing.value = false
}

function startPush(from: LiveSnap, to: LiveSnap | null) {
  if (prefersReducedMotion() || !from.text.trim()) {
    stopPush()
    return
  }
  stopPush()
  departing.value = from
  incoming.value = to
  pushing.value = true
  slideTimer = setTimeout(() => {
    departing.value = null
    incoming.value = null
    pushing.value = false
    slideTimer = null
  }, SLIDE_MS)
}

watch(
  () => [props.liveKey, props.liveLine, props.liveToolName, props.expanded, props.forceLiveSlot] as const,
  () => {
    // Expanded usually clears the live slot; forceLiveSlot keeps thinking after ask_user.
    if (props.expanded && props.forceLiveSlot !== true) return
    const key = props.liveKey ?? null
    const next = snapFromProps()
    const force = props.forceLiveSlot === true

    // Push when the keyed current-task changes (tool ↔ 思考中).
    // Same key (thinking dots) updates the line in place.
    if (key && next.text) {
      if (resting.value.text && restingKey.value !== key) {
        startPush(resting.value, next)
      }
      restingKey.value = key
      resting.value = next
      return
    }

    if (force) {
      if (!resting.value.text && next.text) {
        restingKey.value = key
        resting.value = next
      }
      return
    }

    if (resting.value.text) {
      startPush(resting.value, null)
      restingKey.value = null
      resting.value = { text: '', toolName: null }
    }
  },
  { immediate: true }
)

onUnmounted(() => {
  if (slideTimer != null) window.clearTimeout(slideTimer)
})

const showLiveSlot = computed(() => {
  // Expanded process lists already show tool rows; still allow an explicit
  // live slot (thinking gap after ask_user) when forceLiveSlot is set.
  if (props.expanded && props.forceLiveSlot !== true) return false
  return pushing.value || !!resting.value.text
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
    class="tool-call-trigger group flex flex-col items-start gap-0.5 w-full min-w-0 max-w-full text-left py-0.5 transition cursor-pointer"
    :aria-expanded="expanded"
    :aria-label="ariaLabel || summaryLine"
    @click="emit('toggle')"
  >
    <span
      v-if="showSummaryRow"
      class="collapsed-run-hover-pill collapsed-run-summary-pill"
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
            v-if="incoming"
            class="collapsed-run-live-line collapsed-run-live-incoming"
          >
            <span class="collapsed-run-hover-pill">
              <ToolKindIcon v-if="incoming.toolName" :name="incoming.toolName" />
              <span class="collapsed-run-live-text">{{ incoming.text }}</span>
            </span>
          </span>
          <span
            v-if="departing"
            class="collapsed-run-live-line collapsed-run-live-departing"
          >
            <span class="collapsed-run-hover-pill">
              <ToolKindIcon v-if="departing.toolName" :name="departing.toolName" />
              <span class="collapsed-run-live-text">{{ departing.text }}</span>
            </span>
          </span>
        </template>
        <span
          v-else
          class="collapsed-run-live-line"
        >
          <span class="collapsed-run-hover-pill">
            <ToolKindIcon v-if="resting.toolName" :name="resting.toolName" />
            <ToolLiveSweepText
              class="collapsed-run-live-text"
              :text="resting.text"
              :active="liveBusy"
            />
          </span>
        </span>
      </span>
    </span>
  </button>
</template>
