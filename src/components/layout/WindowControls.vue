<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { Minus, Square, X, Copy } from 'lucide-vue-next'

const { t } = useI18n()

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
  <div class="window-controls flex h-10 shrink-0 items-stretch" role="toolbar" :aria-label="t('shell.window.ariaLabel')">
    <button
      type="button"
      class="window-controls-btn"
      :title="t('shell.window.minimize')"
      :aria-label="t('shell.window.minimize')"
      @click="emit('minimize')"
    >
      <Minus class="w-3.5 h-3.5" stroke-width="2" />
    </button>
    <button
      type="button"
      class="window-controls-btn"
      :title="maximized ? t('shell.window.restore') : t('shell.window.maximize')"
      :aria-label="maximized ? t('shell.window.restore') : t('shell.window.maximize')"
      @click="emit('maximize')"
    >
      <Copy v-if="maximized" class="w-3 h-3" stroke-width="2" />
      <Square v-else class="w-3 h-3" stroke-width="2" />
    </button>
    <button
      type="button"
      class="window-controls-btn window-controls-btn-close"
      :title="t('shell.window.close')"
      :aria-label="t('shell.window.close')"
      @click="emit('close')"
    >
      <X class="w-3.5 h-3.5" stroke-width="2" />
    </button>
  </div>
</template>
