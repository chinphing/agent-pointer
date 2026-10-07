<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

import { computed, ref } from 'vue'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import type { AgentDef } from '../../../types/chat'
import AgentSkillPicker from '../../skills/AgentSkillPicker.vue'
import { useSettingsStore } from '../../../stores/settings'
import { useSkillsStore } from '../../../stores/skills'
import { Bot, RotateCcw, Sparkles, Users } from 'lucide-vue-next'
import { composerAgentLabel } from '../../../lib/agentUi'
import {
  thinkingIntensityOptions,
  patchTierThinkingIntensity,
  tierThinkingIntensityValue
} from '../../../lib/thinkingIntensity'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const s = useSettingsStore()
const skillsStore = useSkillsStore()
const THINKING_INTENSITY_OPTIONS = computed(() => thinkingIntensityOptions())
const skillPickerAgent = ref<AgentDef | null>(null)
const skillPickerAgentName = computed(() =>
  skillPickerAgent.value ? composerAgentLabel(skillPickerAgent.value, s.settings) : ''
)
const {
  enabledWorkers,
  isModeAgent,
  showDebugMenus,
  platformReadOnly,
  getAgentModelWithProvider,
  selectAgentModelWithProvider,
  taskBoardTrimChecked,
  setTaskBoardTrimLocal,
  PERFORMANCE_MODE_UI,
  agentModeLlm,
  selectAgentModeModel,
  patchAgentModeLlm,
  COMPUTER_TIER_UI,
  computerTierLlm,
  patchComputerTierLlm,
  computerTierModelValue,
  selectComputerTierModel,
  computerPipelineLlm,
  patchComputerPipelineLlm,
  computerPipelineVerifyValue,
  selectComputerPipelineVerify,
  MEDIA_DEBUG_KINDS,
  mediaModeLlm,
  selectMediaModeModel,
  patchMediaModeLlm,
  maxSubAgentSpawnDepth,
  isLeadWorkerSelected,
  isLeadAgentSelectable,
  selectLeadWorker
} = props.form

function mediaDebugModelOptions(kind: (typeof MEDIA_DEBUG_KINDS)[number]) {
  if (kind === 'audio') return s.audioModels
  return s.visionModels
}

function supportsSkills(agent: AgentDef): boolean {
  return agent.defaultSkillIds.length > 0
}

function configuredSkillIds(agent: AgentDef): string[] {
  return skillsStore.enabledIdsForAgent(agent.id)
}

function skillLabel(skillId: string): string {
  return skillsStore.skills.find(skill => skill.id === skillId)?.name ?? skillId
}
</script>

<template>            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Bot class="w-4 h-4 text-accent" />{{ t('settings.agentPanel.smartMode') }}
              </h3>
              <p class="mt-0.5 text-xs text-muted">{{ t('settings.agentPanel.smartModeHint') }}</p>
            </div>

            <!-- Agent Cards -->
            <div class="space-y-2">
              <h4 class="text-[12px] font-medium text-muted uppercase tracking-wider">{{ t('settings.agentPanel.execAgents') }}</h4>

              <!-- Worker Agents -->
              <div
                v-for="w in enabledWorkers"
                :key="w.id"
                class="rounded-xl border p-3 transition-all"
                :class="[
                  isLeadWorkerSelected(w.id) ? 'border-border bg-hover' : 'border-border bg-[hsl(var(--card-elevated))]',
                  isLeadAgentSelectable(w) ? 'cursor-pointer hover:border-border' : ''
                ]"
                @click="selectLeadWorker(w)"
              >
                <div class="flex items-start gap-3">
                  <!-- Icon -->
                  <div class="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                       :class="isLeadWorkerSelected(w.id) ? 'bg-hover' : 'bg-[hsl(var(--card-elevated))]'">
                    <Bot class="w-4 h-4" :class="isLeadWorkerSelected(w.id) ? 'text-foreground' : 'text-muted'" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-foreground">{{ composerAgentLabel(w, s.settings) }}</span>
                      <span class="px-1.5 py-0.5 rounded border border-border bg-[hsl(var(--card-elevated))] text-[10px] text-muted font-mono">{{ w.name }}</span>
                      <span v-if="!isLeadAgentSelectable(w)" class="px-1.5 py-0.5 rounded border border-border bg-[hsl(var(--card-elevated))] text-[10px] text-muted">{{ t('settings.agentPanel.subAgents') }}</span>
                      <span v-else-if="isLeadWorkerSelected(w.id)" class="px-1.5 py-0.5 rounded bg-hover text-[10px] font-medium text-foreground">{{ t('settings.agentPanel.selected') }}</span>
                    </div>
                    <p class="mt-0.5 text-[11px] text-muted">{{ w.description || t('settings.agentPanel.generalAgentFallback') }}</p>

                    <!-- Per-agent default model (lead or delegated sub-agent runs) -->
                    <div class="mt-2.5 flex flex-wrap items-center gap-x-4 gap-y-2" @click.stop>
                      <div v-if="!isModeAgent(w.id)" class="flex items-center gap-2 min-w-0">
                        <Sparkles class="w-3.5 h-3.5 text-accent shrink-0" />
                        <span class="text-[11px] text-muted shrink-0">{{ t('settings.agentPanel.defaultModel') }}</span>
                        <select
                          :value="getAgentModelWithProvider(w.id)"
                          @change.stop="selectAgentModelWithProvider(w.id, ($event.target as HTMLSelectElement).value)"
                          @click.stop
                          class="w-48 h-7 px-2 rounded bg-card border border-border text-[11px] text-foreground cursor-pointer outline-none focus:border-accent/50 transition-colors"
                        >
                          <option value="">{{ t('settings.agentPanel.useGlobalDefault') }}</option>
                          <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                        </select>
                      </div>

                      <label class="inline-flex items-center gap-1.5 cursor-pointer shrink-0">
                        <input
                          type="checkbox"
                          class="rounded border-border bg-card text-accent focus:ring-accent/40"
                          :checked="taskBoardTrimChecked(w.id)"
                          @change="setTaskBoardTrimLocal(w.id, ($event.target as HTMLInputElement).checked)"
                        />
                        <span class="text-[11px] text-muted">{{ t('settings.agentPanel.compactAfterBoard') }}</span>
                      </label>

                    </div>

                    <div v-if="supportsSkills(w)" class="mt-2.5 flex flex-wrap items-center gap-2" @click.stop>
                      <Sparkles class="w-3.5 h-3.5 text-accent shrink-0" />
                      <span class="text-[11px] text-muted shrink-0">{{ t('skills.title') }}</span>
                      <span
                        v-if="!skillsStore.hasAgentOverride(w.id)"
                        class="text-[11px] text-muted"
                      >{{ t('settings.agentPanel.inheritGlobal') }}</span>
                      <template v-else-if="configuredSkillIds(w).length">
                        <span
                          v-for="skillId in configuredSkillIds(w).slice(0, 4)"
                          :key="skillId"
                          class="max-w-32 truncate rounded border border-border bg-[hsl(var(--code-bg))] px-1.5 py-0.5 text-[10px] text-foreground/80"
                        >{{ skillLabel(skillId) }}</span>
                        <span v-if="configuredSkillIds(w).length > 4" class="text-[10px] text-muted">
                          +{{ configuredSkillIds(w).length - 4 }}
                        </span>
                      </template>
                      <span v-else class="text-[11px] text-muted">{{ t('settings.agentPanel.noSkills') }}</span>
                      <button
                        type="button"
                        class="h-7 px-2 rounded border border-border bg-card hover:bg-hover text-[11px] text-foreground transition-colors"
                        @click.stop="skillPickerAgent = w"
                      >{{ t('settings.models.configure') }}</button>
                      <button
                        v-if="skillsStore.hasAgentOverride(w.id)"
                        type="button"
                        class="h-7 px-2 rounded hover:bg-hover text-[11px] text-muted flex items-center gap-1 transition-colors"
                        @click.stop="skillsStore.resetAgentOverride(w.id)"
                      >
                        <RotateCcw class="w-3 h-3" />
                        {{ t('settings.agentPanel.reset') }}
                      </button>
                    </div>

                    <div
                      v-if="isModeAgent(w.id) && showDebugMenus"
                      class="col-span-full mt-3 rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-3"
                    >
                      <div class="flex items-center justify-between gap-2">
                        <h4 class="text-xs font-medium text-foreground">{{ t('settings.agentPanel.modeModelsDebug') }}</h4>
                        <span class="text-[10px] text-muted">{{ t('settings.agentPanel.modeModelsHint') }}</span>
                      </div>
                      <div
                        v-for="mode in PERFORMANCE_MODE_UI"
                        :key="w.id + '-tier-' + mode.value"
                        class="grid grid-cols-[3rem_1fr_auto_6rem] gap-2 items-center px-2 py-1.5"
                      >
                        <span class="text-[11px] text-muted font-medium">{{ mode.label }}</span>
                        <select
                          :value="agentModeLlm(w.id, mode.value).providerId + ':' + agentModeLlm(w.id, mode.value).model"
                          class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          @change="selectAgentModeModel(w.id, mode.value, ($event.target as HTMLSelectElement).value)"
                        >
                          <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                        </select>
                        <select
                          class="h-8 col-span-2 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          :value="tierThinkingIntensityValue(agentModeLlm(w.id, mode.value))"
                          @change="patchAgentModeLlm(w.id, mode.value, patchTierThinkingIntensity(($event.target as HTMLSelectElement).value as '' | 'off' | 'low' | 'medium' | 'high' | 'max'))"
                        >
                          <option
                            v-for="opt in THINKING_INTENSITY_OPTIONS"
                            :key="opt.value || 'unset'"
                            :value="opt.value"
                          >{{ opt.label }}</option>
                        </select>
                      </div>
                    </div>

                    <div
                      v-if="w.id === 'computer'"
                      class="col-span-full mt-3 rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-3"
                    >
                      <div class="flex items-center justify-between gap-2">
                        <h4 class="text-xs font-medium text-foreground">{{ t('settings.agentPanel.computerModeModelsDebug') }}</h4>
                        <span class="text-[10px] text-muted">{{ t('settings.agentPanel.computerModeModelsHint') }}</span>
                      </div>
                      <div
                        v-for="tier in COMPUTER_TIER_UI"
                        :key="tier.key"
                        class="grid grid-cols-[4.5rem_1fr_auto_6rem] gap-2 items-center px-2 py-1.5"
                      >
                        <span class="text-[11px] text-muted font-medium">{{ tier.label }}</span>
                        <select
                          :value="computerTierModelValue(tier.key)"
                          class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          @change="selectComputerTierModel(tier.key, ($event.target as HTMLSelectElement).value)"
                        >
                          <option v-for="item in s.allModels" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                        </select>
                        <select
                          class="h-8 col-span-2 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                          :value="tierThinkingIntensityValue(computerTierLlm(tier.key))"
                          @change="patchComputerTierLlm(tier.key, patchTierThinkingIntensity(($event.target as HTMLSelectElement).value as '' | 'off' | 'low' | 'medium' | 'high' | 'max'))"
                        >
                          <option
                            v-for="opt in THINKING_INTENSITY_OPTIONS"
                            :key="opt.value || 'unset'"
                            :value="opt.value"
                          >{{ opt.label }}</option>
                        </select>
                      </div>
                      <div class="border-t border-border pt-3 space-y-2">
                        <div class="flex items-center justify-between gap-2 px-2">
                          <h5 class="text-[11px] font-medium text-foreground">{{ t('settings.agentPanel.verifyDebug') }}</h5>
                          <span class="text-[10px] text-muted">{{ t('settings.agentPanel.verifyHint') }}</span>
                        </div>
                        <div class="grid grid-cols-[4.5rem_1fr_auto_6rem] gap-2 items-center px-2 py-1.5">
                          <span class="text-[11px] text-muted font-medium">Verify</span>
                          <select
                            :value="computerPipelineVerifyValue()"
                            class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                            @change="selectComputerPipelineVerify(($event.target as HTMLSelectElement).value)"
                          >
                            <option v-for="item in s.allModels" :key="'verify-' + item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                          </select>
                          <span class="text-[11px] text-muted whitespace-nowrap">{{ t('settings.agentPanel.thinkingBudget') }}</span>
                          <input
                            type="number"
                            min="256"
                            step="256"
                            class="h-8 w-full px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                            :value="computerPipelineLlm().verifyThinkingBudget ?? 256"
                            @change="patchComputerPipelineLlm({ verifyThinkingBudget: Number(($event.target as HTMLInputElement).value) })"
                          />
                        </div>
                      </div>
                    </div>

                  </div>
                </div>
              </div>

              <div
                v-if="showDebugMenus"
                class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-4 space-y-3"
              >
                <div class="flex items-center justify-between gap-2">
                  <h4 class="text-xs font-medium text-foreground">{{ t('settings.agentPanel.mediaModeModelsDebug') }}</h4>
                  <span class="text-[10px] text-muted">{{ t('settings.agentPanel.mediaModeModelsHint') }}</span>
                </div>
                <div v-for="kind in MEDIA_DEBUG_KINDS" :key="'media-debug-' + kind" class="space-y-2">
                  <span class="text-[12px] text-foreground font-medium">{{ kind === 'image' ? t('settings.mediaKinds.imageShort') : kind === 'audio' ? t('settings.mediaKinds.audioShort') : t('settings.mediaKinds.videoShort') }}</span>
                  <div
                    v-for="mode in PERFORMANCE_MODE_UI"
                    :key="kind + '-' + mode.value"
                    class="grid grid-cols-[3rem_1fr_auto_6rem] gap-2 items-center px-2 py-1.5"
                  >
                    <span class="text-[11px] text-muted font-medium">{{ mode.label }}</span>
                    <select
                      :value="mediaModeLlm(kind, mode.value).providerId + ':' + mediaModeLlm(kind, mode.value).model"
                      class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                      @change="selectMediaModeModel(kind, mode.value, ($event.target as HTMLSelectElement).value)"
                    >
                      <option v-for="item in mediaDebugModelOptions(kind)" :key="item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                    </select>
                    <select
                      class="h-8 col-span-2 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                      :value="tierThinkingIntensityValue(mediaModeLlm(kind, mode.value))"
                      @change="patchMediaModeLlm(kind, mode.value, patchTierThinkingIntensity(($event.target as HTMLSelectElement).value as '' | 'off' | 'low' | 'medium' | 'high' | 'max'))"
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
            </div>

            <div
              class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3"
            >
              <h4 class="text-sm font-medium text-foreground">{{ t('settings.agentPanel.delegation') }}</h4>
              <p class="text-[11px] text-muted">
                {{ t('settings.agentPanel.delegationHintBefore') }} <code class="text-muted">allowAgents</code> {{ t('settings.agentPanel.delegationHintAfter') }}
              </p>
              <div>
                <label class="block text-[12px] text-muted mb-1.5">{{ t('settings.agentPanel.maxNestDepth') }}</label>
                <p class="text-[11px] text-muted mb-1.5">{{ t('settings.agentPanel.maxNestDepthHint') }}</p>
                <input
                  v-model.number="maxSubAgentSpawnDepth"
                  type="number"
                  min="1"
                  max="8"
                  step="1"
                  class="w-full max-w-xs h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
            </div>

            <AgentSkillPicker
              v-if="skillPickerAgent"
              :agent="skillPickerAgent"
              :agent-name="skillPickerAgentName"
              @close="skillPickerAgent = null"
            />


</template>
