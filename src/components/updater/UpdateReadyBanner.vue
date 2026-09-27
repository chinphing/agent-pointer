<script setup lang="ts">
import { X, RefreshCw } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

defineProps<{
  version: string | null
  notes: string | null
}>()

const emit = defineEmits<{
  (e: 'apply'): void
  (e: 'dismiss'): void
  (e: 'skipVersion'): void
}>()
</script>

<template>
  <div
    class="fixed bottom-4 right-4 z-[400] w-[min(380px,calc(100vw-32px))] glass-strong rounded-xl border border-accent/30 shadow-lg p-4 flex flex-col gap-3"
  >
    <div class="flex items-start justify-between gap-2">
      <div class="flex items-center gap-2">
        <div class="w-7 h-7 rounded-lg bg-accent/15 flex items-center justify-center">
          <RefreshCw class="w-4 h-4 text-accent" />
        </div>
        <h3 class="text-sm font-semibold text-foreground">
          {{ t('updaterBanner.title', { version }) }}
        </h3>
      </div>
      <button
        class="h-6 w-6 rounded-md hover:bg-hover inline-flex items-center justify-center cursor-pointer shrink-0"
        @click="emit('dismiss')"
      >
        <X class="w-3.5 h-3.5 text-accent" />
      </button>
    </div>

    <p class="text-xs text-muted leading-relaxed">
      {{ t('updaterBanner.restartHint') }}
    </p>

    <p v-if="notes" class="text-xs text-muted leading-relaxed line-clamp-2">
      {{ notes }}
    </p>

    <div class="flex items-center gap-2">
      <button
        class="h-8 px-4 rounded-lg bg-accent text-accent-foreground text-xs font-medium cursor-pointer hover:opacity-95 transition-opacity"
        @click="emit('apply')"
      >
        {{ t('updaterBanner.updateNow') }}
      </button>
      <button
        class="h-8 px-3 rounded-lg bg-hover text-foreground text-xs cursor-pointer hover:bg-hover/80 transition-colors"
        @click="emit('dismiss')"
      >
        {{ t('updaterBanner.later') }}
      </button>
      <button
        class="h-8 px-3 rounded-lg text-muted text-xs cursor-pointer hover:text-foreground transition-colors ml-auto"
        @click="emit('skipVersion')"
      >
        {{ t('updaterBanner.skipVersion') }}
      </button>
    </div>
  </div>
</template>
