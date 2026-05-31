<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'

const props = defineProps<{
  active: boolean
}>()

const dotPhase = ref(1)

let timer: ReturnType<typeof setInterval> | null = null

const label = computed(() => `思考中${'.'.repeat(dotPhase.value)}`)

function startAnimation() {
  if (timer != null) return
  dotPhase.value = 1
  timer = setInterval(() => {
    dotPhase.value = dotPhase.value >= 3 ? 1 : dotPhase.value + 1
  }, 500)
}

function stopAnimation() {
  if (timer != null) {
    clearInterval(timer)
    timer = null
  }
}

watch(
  () => props.active,
  active => {
    if (active) startAnimation()
    else stopAnimation()
  },
  { immediate: true }
)

onUnmounted(stopAnimation)
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
