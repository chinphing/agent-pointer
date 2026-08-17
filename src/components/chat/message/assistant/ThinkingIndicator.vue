<script setup lang="ts">
import { computed } from 'vue'
import {
  thinkingDotsAtCap,
  thinkingSteadyDotCount,
  THINKING_LIVE_DOT_COUNT
} from '../../../../lib/thinkingIndicator'

const props = defineProps<{
  active: boolean
  charCount?: number
}>()

const charCount = computed(() => props.charCount ?? 0)

const atCap = computed(() => props.active && thinkingDotsAtCap(charCount.value))

const steadyDots = computed(() => {
  if (!props.active) return ''
  return '.'.repeat(thinkingSteadyDotCount(charCount.value))
})
</script>

<template>
  <div
    v-if="active"
    class="text-[13px] text-muted px-3 py-1.5 select-none"
    role="status"
    aria-live="polite"
    :aria-label="atCap ? '思考中，仍在执行' : undefined"
  >
    思考中{{ steadyDots }}<span
      v-if="atCap"
      class="thinking-dots-live"
      aria-hidden="true"
    ><span
      v-for="i in THINKING_LIVE_DOT_COUNT"
      :key="i"
      :style="{ animationDelay: `${(i - 1) * 0.2}s` }"
    >.</span></span>
  </div>
</template>

<style scoped>
.thinking-dots-live span {
  display: inline;
  animation: thinking-dot-wave 1.2s ease-in-out infinite;
}

@keyframes thinking-dot-wave {
  0%,
  100% {
    opacity: 0.2;
  }
  40%,
  55% {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .thinking-dots-live span {
    animation: none;
    opacity: 1;
  }
}
</style>
