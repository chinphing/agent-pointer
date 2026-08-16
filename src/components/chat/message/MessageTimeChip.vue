<script setup lang="ts">
import { computed } from 'vue'
import { Clock } from 'lucide-vue-next'
import { formatMessageTimeClock, formatMessageTimeFull } from '../../../lib/formatMessageTime'

const props = defineProps<{ createdAt: number }>()

const label = computed(() => formatMessageTimeClock(props.createdAt))
const title = computed(() => formatMessageTimeFull(props.createdAt))
</script>

<template>
  <span
    class="inline-flex items-center gap-0.5 text-[11px] text-muted tabular-nums tracking-tight select-none cursor-default"
    :title="title"
  >
    <Clock class="w-3 h-3 shrink-0 opacity-80" aria-hidden="true" />
    {{ label }}
  </span>
</template>
