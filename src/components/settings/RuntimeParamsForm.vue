<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { RuntimeParamsApi } from '../../composables/useRuntimeParams'
import { thinkingIntensityOptions } from '../../lib/thinkingIntensity'

const { t } = useI18n()
const THINKING_INTENSITY_OPTIONS = computed(() => thinkingIntensityOptions())

const props = defineProps<{
  api: RuntimeParamsApi
}>()

const extraBodyDraft = ref('')
const extraBodyError = ref('')

watch(
  () => props.api.extraBodyJson(),
  value => {
    extraBodyDraft.value = value
    extraBodyError.value = ''
  },
  { immediate: true }
)

function commitExtraBody() {
  const result = props.api.setExtraBodyJson(extraBodyDraft.value)
  if (!result.ok) {
    extraBodyError.value = result.error
    console.warn('[settings] extraBody JSON invalid', result.error)
    return
  }
  extraBodyError.value = ''
  extraBodyDraft.value = props.api.extraBodyJson()
}
</script>

<template>
  <div class="space-y-3">
    <!-- Row 1: max output + thinking controls -->
    <div class="flex flex-nowrap items-center gap-x-3 gap-y-2 overflow-x-auto">
      <div class="flex items-center gap-1.5 shrink-0">
        <span class="text-[12px] text-muted whitespace-nowrap">{{ t('settings.runtimeParams.maxOutput') }}</span>
        <input
          type="number"
          min="64"
          max="64000"
          step="64"
          class="w-[4.25rem] h-8 px-1.5 rounded-lg bg-card border border-border text-foreground text-[12px] text-right font-mono outline-none focus:border-accent/50"
          :value="api.maxTokens()"
          @input="api.setMaxTokens(Number(($event.target as HTMLInputElement).value))"
        />
      </div>
      <div class="flex items-center gap-1.5 shrink-0">
        <span class="text-[12px] text-muted whitespace-nowrap">{{ t('settings.runtimeParams.context') }}</span>
        <input
          type="number"
          min="4096"
          max="2097152"
          step="1024"
          class="w-[5.5rem] h-8 px-1.5 rounded-lg bg-card border border-border text-foreground text-[12px] text-right font-mono outline-none focus:border-accent/50"
          :value="api.contextBudgetTokens()"
          @input="api.setContextBudgetTokens(Number(($event.target as HTMLInputElement).value))"
        />
      </div>

      <div class="flex items-center gap-1.5 shrink-0">
        <span class="text-[12px] text-muted whitespace-nowrap">{{ t('settings.runtimeParams.thinking') }}</span>
        <select
          class="h-8 px-1.5 rounded-lg bg-card border border-border text-foreground text-[12px] outline-none focus:border-accent/50"
          :value="api.thinkingIntensity()"
          @change="api.setThinkingIntensity(($event.target as HTMLSelectElement).value)"
        >
          <option
            v-for="opt in THINKING_INTENSITY_OPTIONS"
            :key="opt.value || 'unset'"
            :value="opt.value"
          >{{ opt.label }}</option>
        </select>
      </div>
    </div>

    <!-- Row 2: creativity / top_p / reasoning in messages -->
    <div class="flex flex-nowrap items-center gap-x-3 gap-y-2">
      <div class="flex-1 min-w-0 max-w-[min(100%,18rem)] space-y-1.5">
        <div class="flex items-center justify-between gap-2">
          <span class="text-[12px] text-muted">{{ t('settings.runtimeParams.creativity') }}</span>
          <span class="text-sm font-mono text-accent">{{ api.temperature().toFixed(1) }}</span>
        </div>
        <input
          type="range"
          min="0"
          max="2"
          step="0.1"
          class="w-full accent-accent"
          :value="api.temperature()"
          @input="api.setTemperature(Number(($event.target as HTMLInputElement).value))"
        />
      </div>
      <div class="flex-1 min-w-0 max-w-[min(100%,18rem)] space-y-1.5">
        <div class="flex items-center justify-between gap-2">
          <span class="text-[12px] text-muted">top_p</span>
          <span class="text-sm font-mono text-accent">{{ api.topP().toFixed(2) }}</span>
        </div>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          class="w-full accent-accent"
          :value="api.topP()"
          @input="api.setTopP(Number(($event.target as HTMLInputElement).value))"
        />
      </div>
      <div class="flex items-center gap-1.5 shrink-0">
        <span class="text-[12px] text-muted whitespace-nowrap">{{ t('settings.runtimeParams.returnReasoning') }}</span>
        <label class="relative inline-flex items-center cursor-pointer shrink-0">
          <input
            type="checkbox"
            class="sr-only peer"
            :checked="api.reasoningOn()"
            @change="api.setReasoningOn(($event.target as HTMLInputElement).checked)"
          />
          <div class="settings-toggle-track" />
        </label>
      </div>
    </div>

    <!-- Hermes-style extra_body: flattened to request root on wire -->
    <div class="space-y-1.5">
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-[12px] text-muted">{{ t('settings.runtimeParams.extraBody') }}</span>
        <span class="text-[10px] text-muted/80">{{ t('settings.runtimeParams.extraBodyHint') }}</span>
      </div>
      <textarea
        v-model="extraBodyDraft"
        rows="4"
        spellcheck="false"
        placeholder='{ "repetition_penalty": 1.1, "top_p": 0.8 }'
        class="w-full min-h-[5.5rem] px-2.5 py-2 rounded-lg bg-card border border-border text-foreground text-[11px] font-mono leading-relaxed outline-none focus:border-accent/50 resize-y"
        :class="extraBodyError ? 'border-danger/60' : ''"
        @blur="commitExtraBody"
      />
      <p v-if="extraBodyError" class="text-[11px] text-danger">{{ extraBodyError }}</p>
    </div>
  </div>
</template>
