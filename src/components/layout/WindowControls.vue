<script setup lang="ts">
import { Minus, Square, X, Copy } from 'lucide-vue-next'

defineProps<{
  maximized: boolean
}>()

const emit = defineEmits<{
  (e: 'minimize'): void
  (e: 'maximize'): void
  (e: 'close'): void
}>()
</script>

<template>
  <div class="window-controls flex h-10 shrink-0 items-stretch" role="toolbar" aria-label="窗口">
    <button
      type="button"
      class="window-controls-btn"
      title="最小化"
      aria-label="最小化"
      @click="emit('minimize')"
    >
      <Minus class="w-3.5 h-3.5" stroke-width="2" />
    </button>
    <button
      type="button"
      class="window-controls-btn"
      :title="maximized ? '还原' : '最大化'"
      :aria-label="maximized ? '还原' : '最大化'"
      @click="emit('maximize')"
    >
      <Copy v-if="maximized" class="w-3 h-3" stroke-width="2" />
      <Square v-else class="w-3 h-3" stroke-width="2" />
    </button>
    <button
      type="button"
      class="window-controls-btn window-controls-btn-close"
      title="关闭"
      aria-label="关闭"
      @click="emit('close')"
    >
      <X class="w-3.5 h-3.5" stroke-width="2" />
    </button>
  </div>
</template>
