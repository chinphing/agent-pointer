<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ProviderConfig } from '../../types/chat'
import { resolvedModelCapabilities } from '../../lib/modelCapabilities'

const { t } = useI18n()

const props = defineProps<{
  provider: ProviderConfig
  modelId: string
}>()

const emit = defineEmits<{
  patch: [
    flag: 'supportsVision' | 'supportsAudio' | 'canGenerateImage' | 'canGenerateVideo',
    value: boolean
  ]
}>()

const caps = computed(() =>
  resolvedModelCapabilities([props.provider], props.provider.id, props.modelId)
)

function patch(flag: 'supportsVision' | 'supportsAudio' | 'canGenerateImage' | 'canGenerateVideo', value: boolean) {
  emit('patch', flag, value)
}
</script>

<template>
  <div class="rounded-lg border border-border bg-[hsl(var(--card-elevated))] p-3 space-y-2">
    <p class="text-[12px] font-medium text-foreground">{{ t('settings.capability.heading') }}</p>
    <p class="text-[11px] text-muted">
      {{ t('settings.capability.hint') }}
    </p>
    <label class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-foreground">
      <input
        type="checkbox"
        class="rounded border-border bg-card text-accent focus:ring-accent/40"
        :checked="caps.supportsVision"
        @change="patch('supportsVision', ($event.target as HTMLInputElement).checked)"
      />
      {{ t('settings.capability.vision') }}
    </label>
    <label class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-foreground">
      <input
        type="checkbox"
        class="rounded border-border bg-card text-accent focus:ring-accent/40"
        :checked="caps.supportsAudio"
        @change="patch('supportsAudio', ($event.target as HTMLInputElement).checked)"
      />
      {{ t('settings.capability.audio') }}
    </label>
    <label class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-foreground">
      <input
        type="checkbox"
        class="rounded border-border bg-card text-accent focus:ring-accent/40"
        :checked="caps.canGenerateImage"
        @change="patch('canGenerateImage', ($event.target as HTMLInputElement).checked)"
      />
      {{ t('settings.capability.imageGen') }}
    </label>
    <label class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-foreground">
      <input
        type="checkbox"
        class="rounded border-border bg-card text-accent focus:ring-accent/40"
        :checked="caps.canGenerateVideo"
        @change="patch('canGenerateVideo', ($event.target as HTMLInputElement).checked)"
      />
      {{ t('settings.capability.videoGen') }}
    </label>
  </div>
</template>
