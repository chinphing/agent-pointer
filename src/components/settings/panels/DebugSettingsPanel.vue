<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'

const { t } = useI18n()

const props = defineProps<{
  form: SettingsDialogForm
}>()

const { debugDumpLlmPrompts, rawContentViewEnabled, computerAnnotatedScreenViewEnabled } = props.form
</script>

<template>
  <section class="space-y-5">
    <div class="rounded-xl border border-border panel p-5">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-foreground">{{ t('settings.debug.saveRequests') }}</p>
          <p class="mt-1 text-xs text-muted">{{ t('settings.debug.saveRequestsHint') }}</p>
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
          <p class="text-sm font-medium text-foreground">{{ t('settings.debug.rawContent') }}</p>
          <p class="mt-1 text-xs text-muted">{{ t('settings.debug.rawContentHint') }}</p>
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
          <p class="text-sm font-medium text-foreground">{{ t('settings.debug.annotatedScreen') }}</p>
          <p class="mt-1 text-xs text-muted">{{ t('settings.debug.annotatedScreenHint') }}</p>
        </div>
        <label class="relative inline-flex items-center cursor-pointer shrink-0">
          <input v-model="computerAnnotatedScreenViewEnabled" type="checkbox" class="sr-only peer" />
          <div class="settings-toggle-track" />
        </label>
      </div>
    </div>
  </section>
</template>
