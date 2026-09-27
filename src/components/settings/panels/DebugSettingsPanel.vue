<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const { t } = useI18n()
const { debugDumpLlmPrompts, rawContentViewEnabled, computerAnnotatedScreenViewEnabled } = props.form
</script>

<template>
  <section class="space-y-5">
    <div class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-foreground">{{ t('settings.debug.savePrompts') }}</p>
          <p class="mt-1 text-xs text-muted">{{ t('settings.debug.savePromptsHint') }}</p>
        </div>
        <label class="relative inline-flex items-center cursor-pointer shrink-0">
          <input v-model="debugDumpLlmPrompts" type="checkbox" class="sr-only peer" />
          <div class="settings-toggle-track" />
        </label>
      </div>
    </div>

    <div class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-foreground">{{ t('settings.debug.rawContentView') }}</p>
          <p class="mt-1 text-xs text-muted">{{ t('settings.debug.rawContentViewHint') }}</p>
        </div>
        <label class="relative inline-flex items-center cursor-pointer shrink-0">
          <input v-model="rawContentViewEnabled" type="checkbox" class="sr-only peer" />
          <div class="settings-toggle-track" />
        </label>
      </div>
    </div>

    <div class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-foreground">{{ t('settings.debug.annotatedScreenshotView') }}</p>
          <p class="mt-1 text-xs text-muted">{{ t('settings.debug.annotatedScreenshotViewHint') }}</p>
        </div>
        <label class="relative inline-flex items-center cursor-pointer shrink-0">
          <input v-model="computerAnnotatedScreenViewEnabled" type="checkbox" class="sr-only peer" />
          <div class="settings-toggle-track" />
        </label>
      </div>
    </div>
  </section>
</template>
