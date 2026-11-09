<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { X } from 'lucide-vue-next'
import type { SettingsDialogForm } from '../../composables/useSettingsDialogForm'
import { composerAgentLabel } from '../../lib/agentUi'
import { useSettingsStore } from '../../stores/settings'
import {
  isSameTierRef,
  platformAgentModeDefault,
  platformMediaUnderstandingDefault
} from '../../lib/platformTierDefaults'
import {
  thinkingIntensityOptions,
  patchTierThinkingIntensity,
  tierThinkingIntensityValue
} from '../../lib/thinkingIntensity'

const { t } = useI18n()

const props = defineProps<{
  form: SettingsDialogForm
  /** general | coder | computer | image | audio | video | web_search | explore… */
  scene: string
  open: boolean
}>()

const emit = defineEmits<{ (e: 'close'): void }>()

const s = useSettingsStore()
const form = props.form
const THINKING_INTENSITY_OPTIONS = computed(() => thinkingIntensityOptions())

const scene = computed(() => props.scene)
const isComputer = computed(() => scene.value === 'computer')
// `computer` is a mode agent now: it shares the fast/standard/expert rows with
// general / coder instead of having its own single-model branch.

type AgentMode = Parameters<typeof form.agentModeLlm>[1]

/** `provider:model` for one agent mode row (provider may be empty for built-ins). */
function agentModeValue(agentId: string, mode: string) {
  const c = form.agentModeLlm(agentId, mode as AgentMode)
  return `${c.providerId}:${c.model}`
}

/**
 * True when the resolved row is not in the picker list. Built-in defaults may name
 * a model the current provider does not expose; without this the select renders blank.
 */
function agentModeValueMissing(agentId: string, mode: string) {
  const value = agentModeValue(agentId, mode)
  if (!value || value === ':') return false
  return !s.allModels.some(item => `${item.providerId}:${item.model}` === value)
}

const sceneLabel = computed(() => {
  if (scene.value === 'computer') return t('agents.computer')
  const agent = form.AGENT_MODE_USER_ROWS.value.find(row => row.id === scene.value)
  if (agent) return agent.label
  const media = form.MEDIA_MODE_USER_ROWS.value.find(row => row.key === scene.value)
  if (media) return media.label
  const worker = form.enabledWorkers.value.find(w => w.id === scene.value)
  if (worker) return composerAgentLabel(worker, s.settings)
  return t('settings.sceneModal.sceneFallback')
})

const agentModelOptions = computed(() => s.allModels)

// 统一档位结构：key（档位 id）+ label。agent/media 用 PerformanceMode。
const modeTiers = computed(() =>
  form.PERFORMANCE_MODE_UI.value.map(m => ({ key: m.value, label: m.label }))
)

function patchVariant(
  patch: (value: ReturnType<typeof patchTierThinkingIntensity>) => void,
  value: string
) {
  const intensity =
    value === 'off' || value === 'low' || value === 'medium' || value === 'high' || value === 'max'
      ? value
      : ''
  patch(patchTierThinkingIntensity(intensity))
}

function variantValue(config: {
  thinkingIntensity?: string
  enableThinking?: boolean
  thinkingBudget?: number
  reasoningEffort?: string
}) {
  return tierThinkingIntensityValue(config)
}

function modeOverridden(config: { providerId: string; model: string }, mode: string, group: 'agent' | 'media', id?: string) {
  const tierDefaults = s.platformSettings.tierDefaults
  const fallback =
    (group === 'agent'
      ? platformAgentModeDefault(tierDefaults, id ?? '', mode)
      : platformMediaUnderstandingDefault(tierDefaults, id ?? ''))
  if (!fallback) return Boolean(config.providerId && config.model)
  return !isSameTierRef(config, fallback)
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="pointer-events-auto fixed inset-0 z-[10003] flex items-center justify-center bg-foreground/32 p-4"
      role="presentation"
      @click.self="emit('close')"
    >
      <div class="w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
        <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
          <div class="min-w-0">
            <h4 class="text-sm font-semibold text-foreground">{{ t('settings.sceneModal.title', { scene: sceneLabel }) }}</h4>
            <p class="mt-0.5 text-[11px] text-muted">
              {{ t('settings.sceneModal.hint') }}
            </p>
          </div>
          <button
            type="button"
            class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
            :aria-label="t('common.close')"
            @click="emit('close')"
          >
            <X class="w-4 h-4" />
          </button>
        </div>

        <div class="p-4 space-y-3 overflow-y-auto">
          <div v-for="tier in modeTiers" :key="'agent-' + scene + '-' + tier.key" class="space-y-1">
            <div class="flex items-center gap-1.5">
              <span class="text-[11px] font-medium text-foreground">{{ tier.label }}</span>
              <span v-if="modeOverridden(form.agentModeLlm(scene, tier.key), tier.key, 'agent', scene)" class="rounded bg-warning/15 px-1 text-[9px] text-warning">{{ t('settings.overridden') }}</span>
            </div>
            <div class="grid grid-cols-[minmax(0,1fr)_9.5rem] gap-2 items-center">
              <select
                :value="form.agentModeLlm(scene, tier.key).providerId + ':' + form.agentModeLlm(scene, tier.key).model"
                class="h-8 min-w-0 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                @change="form.selectAgentModeModel(scene, tier.key, ($event.target as HTMLSelectElement).value)"
              >
                <option v-for="item in agentModelOptions" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                <option
                  v-if="agentModeValueMissing(scene, tier.key)"
                  :value="agentModeValue(scene, tier.key)"
                >{{ form.agentModeLlm(scene, tier.key).model }}</option>
              </select>
              <select
                :value="variantValue(form.agentModeLlm(scene, tier.key))"
                class="h-8 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                @change="patchVariant(patch => form.patchAgentModeLlm(scene, tier.key, patch), ($event.target as HTMLSelectElement).value)"
              >
                <option
                  v-for="opt in THINKING_INTENSITY_OPTIONS"
                  :key="opt.value || 'unset'"
                  :value="opt.value"
                >{{ opt.label }}</option>
              </select>
            </div>
          </div>

        </div>

        <div class="border-t border-border px-5 py-3 shrink-0">
          <p class="text-[10px] text-muted">{{ t('settings.sceneModal.liveHint') }}</p>
        </div>
      </div>
    </div>
  </Teleport>
</template>
