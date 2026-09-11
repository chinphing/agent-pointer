<script setup lang="ts">
import { computed, ref } from 'vue'
import { useToolLiveSweep } from '../../composables/useToolLiveSweep'

const props = defineProps<{
  text: string
  active?: boolean
  title?: string
}>()

const root = ref<HTMLElement | null>(null)
const { duration, boxPx } = useToolLiveSweep(
  root,
  () => props.active === true,
  () => props.text
)

const sweepStyle = computed(() => {
  if (!props.active) return undefined
  return {
    '--tool-live-sweep': duration.value,
    '--tool-live-box': boxPx.value > 0 ? `${boxPx.value}px` : undefined,
  }
})
</script>

<template>
  <span
    ref="root"
    :class="{ 'tool-live-pulse': active }"
    :style="sweepStyle"
    :title="title"
  >
    <span class="ellipsis-start-content">{{ text }}</span>
    <span
      v-if="active"
      class="tool-live-pulse-clip"
      aria-hidden="true"
    >
      <span class="tool-live-pulse-sheen">{{ text }}</span>
    </span>
  </span>
</template>
