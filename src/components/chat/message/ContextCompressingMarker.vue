<script setup lang="ts">
import { Loader2, Wrench } from 'lucide-vue-next'

defineProps<{
  label: string
}>()
</script>

<template>
  <div
    class="tool-call-row px-3"
    role="status"
    aria-live="polite"
    data-context-compressing="true"
  >
    <div class="py-1 flex flex-wrap items-center gap-x-1.5 gap-y-0.5 min-w-0 text-[11px] text-muted">
      <Wrench class="w-3 h-3 text-muted/70 shrink-0" />
      <span>{{ label }}</span>
      <span class="shrink-0 inline-flex items-center gap-0.5 text-accent">
        <Loader2 class="w-2.5 h-2.5 animate-spin" />
        <span>压缩中</span>
      </span>
    </div>
  </div>
</template>
