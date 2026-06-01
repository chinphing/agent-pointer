<script setup lang="ts">
import { computed } from 'vue'
import { thinkingDotCount, MAX_THINKING_DOTS } from '../../../../lib/thinkingIndicator'

const props = defineProps<{
  active: boolean
  charCount?: number
}>()

const dotCount = computed(() => {
  if (!props.active) return 0
  const n = props.charCount ?? 0
  return thinkingDotCount(n)
})

const label = computed(() => `思考中${'.'.repeat(dotCount.value)}`)
</script>

<template>
  <div
    v-if="active"
    class="text-[13px] text-muted px-3 py-1.5 select-none"
    role="status"
    aria-live="polite"
  >
    {{ label }}
  </div>
</template>
