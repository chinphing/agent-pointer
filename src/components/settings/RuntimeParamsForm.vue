<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { RuntimeParamsApi } from '../../composables/useRuntimeParams'

const props = defineProps<{
  api: RuntimeParamsApi
}>()

const variant = computed(() => props.api.variant)

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
    <!-- Row 1: max output + provider-specific -->
    <div class="flex flex-nowrap items-center gap-x-3 gap-y-2 overflow-x-auto">
      <div class="flex items-center gap-1.5 shrink-0">
        <span class="text-[12px] text-muted whitespace-nowrap">最大输出</span>
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

      <template v-if="variant === 'qwen'">
        <div class="flex items-center gap-1.5 shrink-0">
          <span class="text-[12px] text-muted whitespace-nowrap">深度思考</span>
          <label class="relative inline-flex items-center cursor-pointer shrink-0">
            <input
              type="checkbox"
              class="sr-only peer"
              :checked="api.deepThinkingOn()"
              @change="api.setDeepThinkingOn(($event.target as HTMLInputElement).checked)"
            />
            <div
              class="settings-toggle-track"
            />
          </label>
        </div>
        <div class="flex items-center gap-1.5 shrink-0">
          <span class="text-[12px] text-muted whitespace-nowrap">思考预算</span>
          <input
            type="number"
            min="1"
            max="64000"
            step="64"
            :disabled="!api.deepThinkingOn()"
            class="w-[4.25rem] h-8 px-1.5 rounded-lg bg-card border border-border text-foreground text-[12px] text-right font-mono outline-none focus:border-accent/50 disabled:opacity-40 disabled:cursor-not-allowed"
            :value="api.thinkingBudget()"
            @input="api.setThinkingBudget(Number(($event.target as HTMLInputElement).value))"
          />
        </div>
      </template>

      <template v-else-if="variant === 'deepseek'">
        <div class="flex items-center gap-1.5 shrink-0">
          <span class="text-[12px] text-muted whitespace-nowrap">推理力度</span>
          <div class="inline-flex rounded-lg bg-card border border-border p-0.5">
            <button
              type="button"
              class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors"
              :class="api.reasoningEffort() === '' ? 'bg-hover text-foreground' : 'text-muted hover:text-foreground'"
              @click="api.setReasoningEffort('')"
            >不设置</button>
            <button
              type="button"
              class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors"
              :class="api.reasoningEffort() === 'high' ? 'bg-hover text-foreground' : 'text-muted hover:text-foreground'"
              @click="api.setReasoningEffort('high')"
            >高力度</button>
            <button
              type="button"
              class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors"
              :class="api.reasoningEffort() === 'max' ? 'bg-hover text-foreground' : 'text-muted hover:text-foreground'"
              @click="api.setReasoningEffort('max')"
            >最大力度</button>
          </div>
        </div>
      </template>
    </div>

    <!-- Row 2: creativity + reasoning in messages -->
    <div class="flex flex-nowrap items-center gap-x-3 gap-y-2">
      <div class="flex-1 min-w-0 max-w-[min(100%,18rem)] space-y-1.5">
        <div class="flex items-center justify-between gap-2">
          <span class="text-[12px] text-muted">创造性</span>
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
      <div class="flex items-center gap-1.5 shrink-0">
        <span class="text-[12px] text-muted whitespace-nowrap">回传推理</span>
        <label class="relative inline-flex items-center cursor-pointer shrink-0">
          <input
            type="checkbox"
            class="sr-only peer"
            :checked="api.reasoningOn()"
            @change="api.setReasoningOn(($event.target as HTMLInputElement).checked)"
          />
          <div
            class="settings-toggle-track"
          />
        </label>
      </div>
    </div>

    <!-- Hermes-style extra_body: flattened to request root on wire -->
    <div class="space-y-1.5">
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-[12px] text-muted">扩展参数 (extra_body)</span>
        <span class="text-[10px] text-muted/80">JSON 对象，发请求时与 temperature 同级</span>
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
