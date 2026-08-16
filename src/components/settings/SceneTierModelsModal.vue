<script setup lang="ts">
import { computed } from 'vue'
import { X } from 'lucide-vue-next'
import type { SettingsDialogForm } from '../../composables/useSettingsDialogForm'
import { detectProviderTemplateId } from '../../lib/providerParams'
import { composerAgentLabel } from '../../lib/agentUi'
import { useSettingsStore } from '../../stores/settings'
import {
  isSameTierRef,
  platformAgentModeDefault,
  platformComputerTierDefault,
  platformMediaModeDefault
} from '../../lib/platformTierDefaults'

const props = defineProps<{
  form: SettingsDialogForm
  /** general | coder | computer | image | audio | video */
  scene: string
  open: boolean
}>()

const emit = defineEmits<{ (e: 'close'): void }>()

const s = useSettingsStore()
const form = props.form
const { PERFORMANCE_MODE_UI, COMPUTER_TIER_UI, AGENT_MODE_USER_ROWS, MEDIA_MODE_USER_ROWS } = form
type MediaKind = (typeof form.MEDIA_DEBUG_KINDS)[number]

const scene = computed(() => props.scene)
const isComputer = computed(() => scene.value === 'computer')
const isMedia = computed(() => (form.MEDIA_DEBUG_KINDS as readonly string[]).includes(scene.value))
const isAgent = computed(() => !isComputer.value && !isMedia.value)
const mediaKind = computed(() => scene.value as MediaKind)

const sceneLabel = computed(() => {
  if (scene.value === 'computer') return '电脑操控'
  const agent = AGENT_MODE_USER_ROWS.find(row => row.id === scene.value)
  if (agent) return agent.label
  const media = MEDIA_MODE_USER_ROWS.find(row => row.key === scene.value)
  if (media) return media.label
  const worker = form.enabledWorkers.value.find(w => w.id === scene.value)
  if (worker) return composerAgentLabel(worker, s.settings)
  return '场景'
})

// 统一档位结构：key（档位 id）+ label。agent/media 用 PerformanceMode，computer 用 ComputerTierKey
const modeTiers = computed(() =>
  PERFORMANCE_MODE_UI.map(m => ({ key: m.value, label: m.label }))
)
const computerTiers = computed(() =>
  COMPUTER_TIER_UI.map(t => ({ key: t.key, label: t.label }))
)

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
  const tierDefaults = s.platformSettings.tierDefaults
  const fallback =
    group === 'computer'
      ? platformComputerTierDefault(tierDefaults, mode)
      : group === 'agent'
        ? platformAgentModeDefault(tierDefaults, id ?? '', mode)
        : platformMediaModeDefault(tierDefaults, id ?? '', mode)
  if (!fallback) return Boolean(config.providerId && config.model)
  return !isSameTierRef(config, fallback)
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="pointer-events-auto fixed inset-0 z-[10002] flex items-center justify-center bg-black/55 p-4"
      role="presentation"
      @click.self="emit('close')"
    >
      <div class="w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
        <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
          <div class="min-w-0">
            <h4 class="text-sm font-semibold text-foreground">{{ sceneLabel }} · 三档模型映射</h4>
            <p class="mt-0.5 text-[11px] text-muted">各档位对应模型与推理参数；「已覆盖」表示非默认模型</p>
          </div>
          <button
            type="button"
            class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
            aria-label="关闭"
            @click="emit('close')"
          >
            <X class="w-4 h-4" />
          </button>
        </div>

        <div class="p-4 space-y-3 overflow-y-auto">
          <!-- agent 场景：通用助手 / 氛围编程 -->
          <template v-if="isAgent">
            <div v-for="tier in modeTiers" :key="'agent-' + scene + '-' + tier.key" class="space-y-1">
              <div class="flex items-center gap-1.5">
                <span class="text-[11px] font-medium text-foreground">{{ tier.label }}</span>
                <span v-if="modeOverridden(form.agentModeLlm(scene, tier.key), tier.key, 'agent', scene)" class="rounded bg-amber-400/15 px-1 text-[9px] text-amber-600">已覆盖</span>
              </div>
              <div class="grid grid-cols-[minmax(0,1fr)_9.5rem] gap-2 items-center">
                <select
                  :value="form.agentModeLlm(scene, tier.key).providerId + ':' + form.agentModeLlm(scene, tier.key).model"
                  class="h-8 min-w-0 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                  @change="form.selectAgentModeModel(scene, tier.key, ($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                </select>
                <div v-if="providerTemplate(form.agentModeLlm(scene, tier.key).providerId, form.agentModeLlm(scene, tier.key).model) === 'qwen'" class="flex items-center gap-1.5 justify-end">
                  <label class="inline-flex items-center gap-1 text-[11px] text-muted whitespace-nowrap">
                    <input type="checkbox" class="accent-accent" :checked="form.agentModeLlm(scene, tier.key).enableThinking !== false" @change="form.patchAgentModeLlm(scene, tier.key, { enableThinking: ($event.target as HTMLInputElement).checked })">思考
                  </label>
                  <input
                    :disabled="form.agentModeLlm(scene, tier.key).enableThinking === false"
                    :value="form.agentModeLlm(scene, tier.key).thinkingBudget ?? 2048"
                    type="number"
                    min="256"
                    step="256"
                    class="h-8 w-16 px-1 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60"
                    aria-label="思考预算"
                    @change="form.patchAgentModeLlm(scene, tier.key, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })"
                  >
                </div>
                <select
                  v-else-if="providerTemplate(form.agentModeLlm(scene, tier.key).providerId, form.agentModeLlm(scene, tier.key).model) === 'deepseek'"
                  :value="variantValue(form.agentModeLlm(scene, tier.key))"
                  class="h-8 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                  @change="patchVariant(form.agentModeLlm(scene, tier.key), patch => form.patchAgentModeLlm(scene, tier.key, patch), ($event.target as HTMLSelectElement).value)"
                >
                  <option value="off">不设置</option>
                  <option value="high">高</option>
                  <option value="max">最高</option>
                </select>
                <span v-else class="text-right text-[11px] text-muted">仅模型</span>
              </div>
            </div>
          </template>

          <!-- media 场景：图片理解 / 语音转写 / 视频理解 -->
          <template v-else-if="isMedia">
            <div v-for="tier in modeTiers" :key="'media-' + scene + '-' + tier.key" class="space-y-1">
              <div class="flex items-center gap-1.5">
                <span class="text-[11px] font-medium text-foreground">{{ tier.label }}</span>
                <span v-if="modeOverridden(form.mediaModeLlm(mediaKind, tier.key), tier.key, 'media', mediaKind)" class="rounded bg-amber-400/15 px-1 text-[9px] text-amber-600">已覆盖</span>
              </div>
              <div class="grid grid-cols-[minmax(0,1fr)_9.5rem] gap-2 items-center">
                <select
                  :value="form.mediaModeLlm(mediaKind, tier.key).providerId + ':' + form.mediaModeLlm(mediaKind, tier.key).model"
                  class="h-8 min-w-0 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                  @change="form.selectMediaModeModel(mediaKind, tier.key, ($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="item in mediaOptions(mediaKind)" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                </select>
                <div v-if="providerTemplate(form.mediaModeLlm(mediaKind, tier.key).providerId, form.mediaModeLlm(mediaKind, tier.key).model) === 'qwen'" class="flex items-center gap-1.5 justify-end">
                  <label class="inline-flex items-center gap-1 text-[11px] text-muted whitespace-nowrap">
                    <input type="checkbox" class="accent-accent" :checked="form.mediaModeLlm(mediaKind, tier.key).enableThinking !== false" @change="form.patchMediaModeLlm(mediaKind, tier.key, { enableThinking: ($event.target as HTMLInputElement).checked })">思考
                  </label>
                  <input
                    :disabled="form.mediaModeLlm(mediaKind, tier.key).enableThinking === false"
                    :value="form.mediaModeLlm(mediaKind, tier.key).thinkingBudget ?? 2048"
                    type="number"
                    min="256"
                    step="256"
                    class="h-8 w-16 px-1 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60"
                    aria-label="思考预算"
                    @change="form.patchMediaModeLlm(mediaKind, tier.key, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })"
                  >
                </div>
                <select
                  v-else-if="providerTemplate(form.mediaModeLlm(mediaKind, tier.key).providerId, form.mediaModeLlm(mediaKind, tier.key).model) === 'deepseek'"
                  :value="variantValue(form.mediaModeLlm(mediaKind, tier.key))"
                  class="h-8 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                  @change="patchVariant(form.mediaModeLlm(mediaKind, tier.key), patch => form.patchMediaModeLlm(mediaKind, tier.key, patch), ($event.target as HTMLSelectElement).value)"
                >
                  <option value="off">不设置</option>
                  <option value="high">高</option>
                  <option value="max">最高</option>
                </select>
                <span v-else class="text-right text-[11px] text-muted">仅模型</span>
              </div>
            </div>
          </template>

          <!-- computer 场景：电脑操控 -->
          <template v-else>
            <div v-for="tier in computerTiers" :key="'computer-' + tier.key" class="space-y-1">
              <div class="flex items-center gap-1.5">
                <span class="text-[11px] font-medium text-foreground">{{ tier.label }}</span>
                <span v-if="modeOverridden(form.computerTierLlm(tier.key), tier.key, 'computer')" class="rounded bg-amber-400/15 px-1 text-[9px] text-amber-600">已覆盖</span>
              </div>
              <div class="grid grid-cols-[minmax(0,1fr)_9.5rem] gap-2 items-center">
                <select
                  :value="form.computerTierModelValue(tier.key)"
                  class="h-8 min-w-0 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                  @change="form.selectComputerTierModel(tier.key, ($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                </select>
                <div v-if="providerTemplate(form.computerTierLlm(tier.key).providerId, form.computerTierLlm(tier.key).model) === 'qwen'" class="flex items-center gap-1.5 justify-end">
                  <label class="inline-flex items-center gap-1 text-[11px] text-muted whitespace-nowrap">
                    <input type="checkbox" class="accent-accent" :checked="form.computerTierLlm(tier.key).enableThinking !== false" @change="form.patchComputerTierLlm(tier.key, { enableThinking: ($event.target as HTMLInputElement).checked })">思考
                  </label>
                  <input
                    :disabled="form.computerTierLlm(tier.key).enableThinking === false"
                    :value="form.computerTierLlm(tier.key).thinkingBudget ?? 2048"
                    type="number"
                    min="256"
                    step="256"
                    class="h-8 w-16 px-1 rounded border border-border bg-card text-[11px] text-foreground disabled:opacity-60"
                    aria-label="思考预算"
                    @change="form.patchComputerTierLlm(tier.key, { thinkingBudget: Number(($event.target as HTMLInputElement).value) })"
                  >
                </div>
                <select
                  v-else-if="providerTemplate(form.computerTierLlm(tier.key).providerId, form.computerTierLlm(tier.key).model) === 'deepseek'"
                  :value="variantValue(form.computerTierLlm(tier.key))"
                  class="h-8 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                  @change="patchVariant(form.computerTierLlm(tier.key), patch => form.patchComputerTierLlm(tier.key, patch), ($event.target as HTMLSelectElement).value)"
                >
                  <option value="off">不设置</option>
                  <option value="high">高</option>
                  <option value="max">最高</option>
                </select>
                <span v-else class="text-right text-[11px] text-muted">仅模型</span>
              </div>
            </div>
          </template>
        </div>

        <div class="border-t border-border px-5 py-3 shrink-0">
          <p class="text-[10px] text-muted">变更即时生效，与「智能体 → 场景档位」联动</p>
        </div>
      </div>
    </div>
  </Teleport>
</template>
