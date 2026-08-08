<script setup lang="ts">
import { BrainCircuit } from 'lucide-vue-next'
import type { SettingsDialogForm } from '../../composables/useSettingsDialogForm'
import { detectProviderTemplateId } from '../../lib/providerParams'
import { useSettingsStore } from '../../stores/settings'

const props = defineProps<{
  form: SettingsDialogForm
  readOnly?: boolean
}>()

const form = props.form
const s = useSettingsStore()
const { PERFORMANCE_MODE_UI, COMPUTER_TIER_UI } = form
type MediaKind = (typeof form.MEDIA_DEBUG_KINDS)[number]

function providerTemplate(providerId: string, model: string) {
  const provider = s.settings.providers.find(item => item.id === providerId)
  return detectProviderTemplateId(provider ?? { id: providerId, baseUrl: '' })
}

function mediaOptions(kind: MediaKind) {
  return kind === 'audio' ? s.audioModels : s.visionModels
}

function patchVariant(
  config: { providerId: string; model: string; enableThinking?: boolean; thinkingBudget?: number },
  patch: (value: { enableThinking?: boolean; thinkingBudget?: number }) => void,
  value: string
) {
  if (value === 'off') {
    patch({ enableThinking: false, thinkingBudget: undefined })
  } else if (value === 'max') {
    patch({ enableThinking: true, thinkingBudget: 8192 })
  } else {
    patch({ enableThinking: true, thinkingBudget: 2048 })
  }
}

function variantValue(config: { enableThinking?: boolean; thinkingBudget?: number }) {
  if (config.enableThinking === false) return 'off'
  return (config.thinkingBudget ?? 2048) >= 8192 ? 'max' : 'high'
}

function modeOverridden(config: { providerId: string; model: string }, mode: string, group: 'agent' | 'media' | 'computer', id?: string) {
  if (group === 'computer') {
    const defaults: Record<string, string> = { primary: 'qwen:qwen3.5-plus', intermediate: 'qwen:qwen3.5-plus', advanced: 'qwen:qwen3.7-plus' }
    return `${config.providerId}:${config.model}` !== defaults[mode]
  }
  if (group === 'agent') {
    const defaults: Record<string, string> = {
      fast: 'deepseek:deepseek-v4-flash',
      standard: 'deepseek:deepseek-v4-pro',
      expert: `qwen:${id === 'coder' ? 'qwen3.7-max' : 'qwen3.7-plus'}`
    }
    return `${config.providerId}:${config.model}` !== defaults[mode]
  }
  const defaults = id === 'audio'
    ? { fast: 'qwen:qwen3-asr-flash', standard: 'qwen:fun-asr', expert: 'qwen:fun-asr' }
    : { fast: 'qwen:qwen3.5-flash', standard: 'qwen:qwen3.5-plus', expert: 'qwen:qwen3.6-plus' }
  return `${config.providerId}:${config.model}` !== (defaults as Record<string, string>)[mode]
}
</script>

<template>
  <section class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-5">
    <div class="flex items-center gap-2">
      <BrainCircuit class="w-4 h-4 text-accent" />
      <div>
        <h4 class="text-sm font-medium text-foreground">三档模型配置</h4>
        <p class="text-[11px] text-muted">快速、标准、高级分别决定对应能力的模型与推理参数。</p>
      </div>
    </div>

    <div class="space-y-3">
      <div v-for="agent in ['general', 'coder']" :key="agent" class="space-y-2">
        <h5 class="text-[12px] font-medium text-foreground">{{ agent === 'general' ? '对话 · 通用助手' : '对话 · 氛围编程' }}</h5>
        <div v-for="mode in PERFORMANCE_MODE_UI" :key="agent + mode.value" class="grid grid-cols-[3rem_minmax(0,1fr)_7rem] gap-2 items-center">
          <div class="flex items-center gap-1"><span class="text-[11px] text-muted">{{ mode.label }}</span><span v-if="modeOverridden(form.agentModeLlm(agent, mode.value), mode.value, 'agent', agent)" class="rounded bg-amber-400/15 px-1 text-[9px] text-amber-600">已覆盖</span></div>
          <select :disabled="readOnly" :value="form.agentModeLlm(agent, mode.value).model" class="h-8 min-w-0 px-2 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60" @change="form.selectAgentModeModel(agent, mode.value, ($event.target as HTMLSelectElement).value)">
            <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.model">{{ item.providerName }} / {{ item.model }}</option>
          </select>
          <div v-if="providerTemplate(form.agentModeLlm(agent, mode.value).providerId, form.agentModeLlm(agent, mode.value).model) === 'qwen'" class="flex items-center gap-1">
            <label class="inline-flex items-center gap-1 text-[11px] text-muted"><input :disabled="readOnly" type="checkbox" :checked="form.agentModeLlm(agent, mode.value).enableThinking !== false" @change="form.patchAgentModeLlm(agent, mode.value, { enableThinking: ($event.target as HTMLInputElement).checked })">思考</label>
            <input :disabled="readOnly || form.agentModeLlm(agent, mode.value).enableThinking === false" :value="form.agentModeLlm(agent, mode.value).thinkingBudget ?? 2048" type="number" min="256" step="256" class="h-8 w-16 px-1 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60" aria-label="思考预算" @change="form.patchAgentModeLlm(agent, mode.value, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })">
          </div>
          <select v-else-if="providerTemplate(form.agentModeLlm(agent, mode.value).providerId, form.agentModeLlm(agent, mode.value).model) === 'deepseek'" :disabled="readOnly" :value="variantValue(form.agentModeLlm(agent, mode.value))" class="h-8 px-2 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60" @change="patchVariant(form.agentModeLlm(agent, mode.value), patch => form.patchAgentModeLlm(agent, mode.value, patch), ($event.target as HTMLSelectElement).value)"><option value="off">不设置</option><option value="high">高</option><option value="max">最高</option></select>
          <span v-else class="text-[11px] text-muted">仅模型</span>
        </div>
      </div>

      <div v-for="kind in form.MEDIA_DEBUG_KINDS" :key="kind" class="space-y-2">
        <h5 class="text-[12px] font-medium text-foreground">{{ kind === 'image' ? '图片理解' : kind === 'audio' ? '语音理解' : '视频理解' }}</h5>
        <div v-for="mode in PERFORMANCE_MODE_UI" :key="kind + mode.value" class="grid grid-cols-[3rem_minmax(0,1fr)_7rem] gap-2 items-center">
          <div class="flex items-center gap-1"><span class="text-[11px] text-muted">{{ mode.label }}</span><span v-if="modeOverridden(form.mediaModeLlm(kind, mode.value), mode.value, 'media', kind)" class="rounded bg-amber-400/15 px-1 text-[9px] text-amber-600">已覆盖</span></div>
          <select :disabled="readOnly" :value="form.mediaModeLlm(kind, mode.value).model" class="h-8 min-w-0 px-2 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60" @change="form.selectMediaModeModel(kind, mode.value, ($event.target as HTMLSelectElement).value)"><option v-for="item in mediaOptions(kind)" :key="item.providerId + ':' + item.model" :value="item.model">{{ item.providerName }} / {{ item.model }}</option></select>
          <div v-if="providerTemplate(form.mediaModeLlm(kind, mode.value).providerId, form.mediaModeLlm(kind, mode.value).model) === 'qwen'" class="flex items-center gap-1"><label class="inline-flex items-center gap-1 text-[11px] text-muted"><input :disabled="readOnly" type="checkbox" :checked="form.mediaModeLlm(kind, mode.value).enableThinking !== false" @change="form.patchMediaModeLlm(kind, mode.value, { enableThinking: ($event.target as HTMLInputElement).checked })">思考</label><input :disabled="readOnly || form.mediaModeLlm(kind, mode.value).enableThinking === false" :value="form.mediaModeLlm(kind, mode.value).thinkingBudget ?? 2048" type="number" min="256" step="256" class="h-8 w-16 px-1 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60" aria-label="思考预算" @change="form.patchMediaModeLlm(kind, mode.value, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })"></div>
          <select v-else-if="providerTemplate(form.mediaModeLlm(kind, mode.value).providerId, form.mediaModeLlm(kind, mode.value).model) === 'deepseek'" :disabled="readOnly" :value="variantValue(form.mediaModeLlm(kind, mode.value))" class="h-8 px-2 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60" @change="patchVariant(form.mediaModeLlm(kind, mode.value), patch => form.patchMediaModeLlm(kind, mode.value, patch), ($event.target as HTMLSelectElement).value)"><option value="off">不设置</option><option value="high">高</option><option value="max">最高</option></select>
          <span v-else class="text-[11px] text-muted">仅模型</span>
        </div>
      </div>

      <div class="space-y-2">
        <h5 class="text-[12px] font-medium text-foreground">电脑操控</h5>
        <div v-for="tier in COMPUTER_TIER_UI" :key="tier.key" class="grid grid-cols-[3rem_minmax(0,1fr)_7rem] gap-2 items-center">
          <div class="flex items-center gap-1"><span class="text-[11px] text-muted">{{ tier.label }}</span><span v-if="modeOverridden(form.computerTierLlm(tier.key), tier.key, 'computer')" class="rounded bg-amber-400/15 px-1 text-[9px] text-amber-600">已覆盖</span></div>
          <select :disabled="readOnly" :value="form.computerTierModelValue(tier.key)" class="h-8 min-w-0 px-2 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60" @change="form.selectComputerTierModel(tier.key, ($event.target as HTMLSelectElement).value)"><option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option></select>
          <div v-if="providerTemplate(form.computerTierLlm(tier.key).providerId, form.computerTierLlm(tier.key).model) === 'qwen'" class="flex items-center gap-1"><label class="inline-flex items-center gap-1 text-[11px] text-muted"><input :disabled="readOnly" type="checkbox" :checked="form.computerTierLlm(tier.key).enableThinking !== false" @change="form.patchComputerTierLlm(tier.key, { enableThinking: ($event.target as HTMLInputElement).checked })">思考</label><input :disabled="readOnly || form.computerTierLlm(tier.key).enableThinking === false" :value="form.computerTierLlm(tier.key).thinkingBudget ?? 2048" type="number" min="256" step="256" class="h-8 w-16 px-1 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60" aria-label="思考预算" @change="form.patchComputerTierLlm(tier.key, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })"></div>
          <select v-else-if="providerTemplate(form.computerTierLlm(tier.key).providerId, form.computerTierLlm(tier.key).model) === 'deepseek'" :disabled="readOnly" :value="variantValue(form.computerTierLlm(tier.key))" class="h-8 px-2 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60" @change="patchVariant(form.computerTierLlm(tier.key), patch => form.patchComputerTierLlm(tier.key, patch), ($event.target as HTMLSelectElement).value)"><option value="off">不设置</option><option value="high">高</option><option value="max">最高</option></select>
          <span v-else class="text-[11px] text-muted">仅模型</span>
        </div>
      </div>
    </div>
  </section>
</template>
