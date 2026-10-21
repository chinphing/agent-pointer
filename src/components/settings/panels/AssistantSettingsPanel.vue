<script setup lang="ts">
import { useI18n } from 'vue-i18n'
const { t } = useI18n()

import { computed, ref } from 'vue'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { useSettingsStore } from '../../../stores/settings'
import { CircleHelp, Code, Film, Gauge, Monitor, SlidersHorizontal, UserRound, Bot, X } from 'lucide-vue-next'
import { composerAgentLabel } from '../../../lib/agentUi'
import SceneTierModelsModal from '../SceneTierModelsModal.vue'
import { budgetForIntensity, intensityFromLegacy, thinkingIntensityOptions } from '../../../lib/thinkingIntensity'
import type { ThinkingIntensity } from '../../../lib/thinkingIntensity'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const s = useSettingsStore()

// 场景三档模型映射弹窗（点场景行「模型」打开）
const sceneModal = ref<string | null>(null)
// 「更多」弹窗：其他执行智能体（explorer 等）的档位与模型设置
const moreModal = ref(false)
// 电脑操控「更多」：pipeline 内部的 Locate / Verify 模型与思考强度
const computerAdvancedModal = ref(false)
// 只保留 off/low/medium/high/max；'' (不设置) 和 off 写的是同一个 0，重复了
const THINKING_OPTIONS = computed(() => thinkingIntensityOptions().filter(o => o.value !== ''))

function intensityOf(budget: number | undefined): string {
  if (!budget) return 'off'
  return intensityFromLegacy({ thinkingBudget: budget }) || ''
}

function setIntensity(phase: 'locate' | 'verify', value: string) {
  const budget = value === 'off' || !value ? 0 : (budgetForIntensity(value as ThinkingIntensity) ?? 0)
  if (phase === 'locate') {
    props.form.patchComputerPipelineLlm({ locateThinkingBudget: budget })
  } else {
    props.form.patchComputerPipelineLlm({ verifyThinkingBudget: budget })
  }
}

// 媒体列「更多」：联网搜索等工具档位（非子智能体，与左侧 explore 入口对称）
const moreMediaModal = ref(false)
const WEB_SEARCH_SCENE_ID = 'web_search'
const RETIRED_MORE_AGENT_IDS = new Set(['research'])
const moreWorkers = computed(() =>
  enabledWorkers.value.filter(
    w =>
      w.id !== 'general' &&
      w.id !== 'coder' &&
      w.id !== 'computer' &&
      !RETIRED_MORE_AGENT_IDS.has(w.id)
  )
)
// 弹窗内短中文描述，风格与左卡「通用助手/氛围编程」一致；未收录的 worker 回退英文原文截断
const WORKER_DESC = computed((): Record<string, string> => ({
  explore: t('settings.assistant.exploreDesc')
}))

const mediaModeDesc = (key: string): string => {
  const descs: Record<string, string> = {
    image: t('settings.assistant.imageDesc'),
    audio: t('settings.assistant.audioDesc'),
    video: t('settings.assistant.videoDesc')
  }
  return descs[key] ?? ''
}

const {
  PERFORMANCE_MODE_HELP,
  MEDIA_MODE_USER_ROWS,
  PERFORMANCE_MODE_UI,
  computerPipelineLocateValue,
  computerPipelineVerifyValue,
  computerPipelineLlm,
  selectComputerPipelineLocate,
  selectComputerPipelineVerify,
  enabledWorkers,
  agentPerformanceModesLocal,
  mediaUnderstandingModesLocal,
  userCodingRules
} = props.form

</script>

<template>
  <div class="flex-1 flex flex-col gap-5">
    <!-- 场景档位 -->
    <section class="space-y-4" aria-labelledby="assistant-scene-heading">
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
        <div>
          <h4 id="assistant-scene-heading" class="text-sm font-medium text-foreground flex items-center gap-2">
            <Gauge class="w-4 h-4 text-accent" />{{ t('settings.assistant.sceneTiers') }}
            <span
              class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
              :title="PERFORMANCE_MODE_HELP"
              :aria-label="t('settings.assistant.tierHelp')"
            >
              <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
            </span>
          </h4>
          <p class="mt-1 text-[11px] text-muted">
            {{ t('settings.assistant.sceneTiersHint') }}
          </p>
        </div>
        <div class="grid grid-cols-2 gap-2.5 items-start">
          <div class="rounded-lg border border-border bg-card/50 overflow-hidden">
            <div class="px-3.5 pt-3 pb-2 border-b border-border flex items-center justify-between gap-2">
              <h5 class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">{{ t('settings.assistant.agentConfig') }}</h5>
              <button
                type="button"
                class="inline-flex items-center h-6 px-1.5 rounded-md text-[11px] text-muted hover:text-foreground hover:bg-hover cursor-pointer transition-colors shrink-0"
                :title="t('settings.assistant.moreAgentsHint')"
                :aria-label="t('settings.assistant.moreAgentsTitle')"
                @click="moreModal = true"
              >
                {{ t('settings.assistant.more') }}
              </button>
            </div>
            <div class="divide-y divide-border">
          <div class="px-3.5 py-3 space-y-2">
            <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <Bot class="w-4 h-4 text-accent shrink-0" />
              <span class="text-[12px] font-medium text-foreground">{{ t('agents.general') }}</span>
              <span class="text-[10px] text-muted">{{ t('settings.assistant.generalDesc') }}</span>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                <label
                  v-for="opt in PERFORMANCE_MODE_UI"
                  :key="'scene-general-' + opt.value"
                  class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                  :class="(agentPerformanceModesLocal.general ?? 'standard') === opt.value
                    ? 'bg-hover text-foreground'
                    : 'text-muted hover:text-foreground'"
                >
                  <input
                    type="radio"
                    class="sr-only"
                    name="scene-general-mode"
                    :checked="(agentPerformanceModesLocal.general ?? 'standard') === opt.value"
                    @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, general: opt.value }"
                  />
                  {{ opt.label }}
                </label>
                </div>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  :title="t('settings.s_8fd4c8')"
                  :aria-label="t('settings.s_a5aef6')"
                  @click="sceneModal = 'general'"
                >
                  <SlidersHorizontal class="w-3 h-3" />
                  {{ t('settings.assistant.models') }}
                </button>
            </div>
          </div>

          <div class="px-3.5 py-3 space-y-2">
            <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <Code class="w-4 h-4 text-accent shrink-0" />
              <span class="text-[12px] font-medium text-foreground">{{ t('agents.coder') }}</span>
              <span class="text-[10px] text-muted">{{ t('settings.assistant.coderDesc') }}</span>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                <label
                  v-for="opt in PERFORMANCE_MODE_UI"
                  :key="'scene-coder-' + opt.value"
                  class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                  :class="(agentPerformanceModesLocal.coder ?? 'standard') === opt.value
                    ? 'bg-hover text-foreground'
                    : 'text-muted hover:text-foreground'"
                >
                  <input
                    type="radio"
                    class="sr-only"
                    name="scene-coder-mode"
                    :checked="(agentPerformanceModesLocal.coder ?? 'standard') === opt.value"
                    @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, coder: opt.value }"
                  />
                  {{ opt.label }}
                </label>
                </div>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  :title="t('settings.s_8fd4c8')"
                  :aria-label="t('settings.s_042d00')"
                  @click="sceneModal = 'coder'"
                >
                  <SlidersHorizontal class="w-3 h-3" />
                  {{ t('settings.assistant.models') }}
                </button>
            </div>
          </div>

          <div class="px-3.5 py-3 space-y-2">
            <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <Monitor class="w-4 h-4 text-accent shrink-0" />
              <span class="text-[12px] font-medium text-foreground">{{ t('agents.computer') }}</span>
              <span class="text-[10px] text-muted">{{ t('settings.assistant.computerDesc') }}</span>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                <label
                  v-for="opt in PERFORMANCE_MODE_UI"
                  :key="'scene-computer-' + opt.value"
                  class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                  :class="(agentPerformanceModesLocal.computer ?? 'standard') === opt.value
                    ? 'bg-hover text-foreground'
                    : 'text-muted hover:text-foreground'"
                >
                  <input
                    type="radio"
                    class="sr-only"
                    name="scene-computer-mode"
                    :checked="(agentPerformanceModesLocal.computer ?? 'standard') === opt.value"
                    @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, computer: opt.value }"
                  />
                  {{ opt.label }}
                </label>
                </div>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  :title="t('settings.s_8fd4c8')"
                  :aria-label="t('settings.s_477f10')"
                  @click="sceneModal = 'computer'"
                >
                  <SlidersHorizontal class="w-3 h-3" />
                  {{ t('settings.assistant.models') }}
                </button>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2 rounded-lg text-[11px] text-muted hover:text-foreground hover:bg-hover cursor-pointer transition-colors shrink-0"
                  :aria-label="t('settings.assistant.more')"
                  @click="computerAdvancedModal = true"
                >
                  {{ t('settings.assistant.more') }}
                </button>
            </div>
          </div>
            </div>
          </div>

          <div class="rounded-lg border border-border bg-card/50 overflow-hidden">
            <div class="px-3.5 pt-3 pb-2 border-b border-border flex items-center justify-between gap-2">
              <h5 class="h-6 flex items-center text-[11px] font-semibold uppercase tracking-wider text-muted/80">{{ t('settings.assistant.mediaUnderstanding') }}</h5>
              <button
                type="button"
                class="inline-flex items-center h-6 px-1.5 rounded-md text-[11px] text-muted hover:text-foreground hover:bg-hover cursor-pointer transition-colors shrink-0"
                :title="t('settings.assistant.moreToolsHint')"
                :aria-label="t('settings.s_da822e')"
                @click="moreMediaModal = true"
              >
                {{ t('settings.assistant.more') }}
              </button>
            </div>
            <div class="divide-y divide-border">
          <div
            v-for="row in MEDIA_MODE_USER_ROWS"
            :key="'scene-media-' + row.key"
            class="px-3.5 py-3 space-y-2"
          >
            <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <Film class="w-4 h-4 text-accent shrink-0" />
              <span class="text-[12px] font-medium text-foreground">{{ row.label }}</span>
              <span class="text-[10px] text-muted">{{ mediaModeDesc(row.key) }}</span>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                <label
                  v-for="opt in PERFORMANCE_MODE_UI"
                  :key="row.key + '-mode-' + opt.value"
                  class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                  :class="mediaUnderstandingModesLocal[row.key] === opt.value
                    ? 'bg-hover text-foreground'
                    : 'text-muted hover:text-foreground'"
                >
                  <input
                    type="radio"
                    class="sr-only"
                    :name="'media-mode-' + row.key"
                    :checked="mediaUnderstandingModesLocal[row.key] === opt.value"
                    @change="mediaUnderstandingModesLocal = { ...mediaUnderstandingModesLocal, [row.key]: opt.value }"
                  />
                  {{ opt.label }}
                </label>
              </div>
              <button
                type="button"
                class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                :title="t('settings.s_8fd4c8')"
                :aria-label="t('settings.assistant.configureTierModelsAria', { name: row.label })"
                @click="sceneModal = row.key"
              >
                <SlidersHorizontal class="w-3 h-3" />
                {{ t('settings.assistant.models') }}
              </button>
            </div>
          </div>
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- 个性化 -->
    <section class="space-y-4" aria-labelledby="assistant-personalize-heading">
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <div>
          <h4 id="assistant-personalize-heading" class="text-sm font-medium text-foreground flex items-center gap-2">
            <UserRound class="w-4 h-4 text-accent" />{{ t('settings.assistant.personalize') }}
          </h4>
          <p class="mt-1 text-[11px] text-muted">
            {{ t('settings.assistant.personalizeHint') }}
          </p>
        </div>
        <textarea
          v-model="userCodingRules"
          rows="5"
          maxlength="4000"
          :placeholder="t('settings.assistant.personalizePlaceholder')"
          class="w-full min-h-[7rem] px-3 py-2 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors resize-y"
        />
        <p class="text-[10px] text-muted text-right">{{ userCodingRules.length }} / 4000</p>
      </div>
    </section>
    <!-- 场景三档模型映射弹窗 -->
    <SceneTierModelsModal :form="form" :scene="sceneModal ?? ''" :open="sceneModal !== null" @close="sceneModal = null" />


    <!-- 电脑操控「更多」弹窗：Locate / Verify 的模型与思考强度 -->
    <Teleport to="body">
      <div
        v-if="computerAdvancedModal"
        class="pointer-events-auto fixed inset-0 z-[10002] flex items-center justify-center bg-foreground/32 p-4"
        role="presentation"
        @click.self="computerAdvancedModal = false"
      >
        <div class="w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
          <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">{{ t('settings.assistant.computerAdvancedTitle') }}</h4>
              <p class="mt-0.5 text-[11px] text-muted">{{ t('settings.assistant.computerAdvancedHint') }}</p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              :aria-label="t('workspace.closeTab')"
              @click="computerAdvancedModal = false"
            >
              <X class="w-4 h-4" />
            </button>
          </div>
          <div class="divide-y divide-border overflow-y-auto">
            <div class="px-3.5 py-3 space-y-2">
              <div class="flex items-center gap-x-2 min-w-0">
                <Monitor class="w-4 h-4 text-accent shrink-0" />
                <span class="text-[12px] font-medium text-foreground shrink-0">{{ t('settings.agentPanel.locateName') }}</span>
                <span class="text-[10px] text-muted truncate min-w-0">{{ t('settings.agentPanel.locateHint') }}</span>
              </div>
              <div class="grid grid-cols-[1fr_5.5rem] gap-2 items-center">
                <select
                  :value="computerPipelineLocateValue()"
                  class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                  @change="selectComputerPipelineLocate(($event.target as HTMLSelectElement).value)"
                >
                  <option value="">{{ t('settings.agentPanel.locateInherit') }}</option>
                  <option v-for="item in s.allModels" :key="'locate-' + item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                </select>
                <select
                  :value="intensityOf(computerPipelineLlm().locateThinkingBudget)"
                  class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                  @change="setIntensity('locate', ($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="opt in THINKING_OPTIONS" :key="'locate-t-' + opt.value" :value="opt.value">{{ opt.label }}</option>
                </select>
              </div>
            </div>
            <div class="px-3.5 py-3 space-y-2">
              <div class="flex items-center gap-x-2 min-w-0">
                <Monitor class="w-4 h-4 text-accent shrink-0" />
                <span class="text-[12px] font-medium text-foreground shrink-0">{{ t('settings.agentPanel.verifyName') }}</span>
                <span class="text-[10px] text-muted truncate min-w-0">{{ t('settings.agentPanel.verifyHint') }}</span>
              </div>
              <div class="grid grid-cols-[1fr_5.5rem] gap-2 items-center">
                <select
                  :value="computerPipelineVerifyValue()"
                  class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                  @change="selectComputerPipelineVerify(($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="item in s.allModels" :key="'verify-' + item.providerId + ':' + item.model" :value="item.providerId + ':' + item.model">{{ item.providerName }} / {{ item.model }}</option>
                </select>
                <select
                  :value="intensityOf(computerPipelineLlm().verifyThinkingBudget)"
                  class="h-8 px-2 rounded border border-border bg-[hsl(var(--card-elevated))] text-[12px] text-foreground outline-none focus:border-accent/50"
                  @change="setIntensity('verify', ($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="opt in THINKING_OPTIONS" :key="'verify-t-' + opt.value" :value="opt.value">{{ opt.label }}</option>
                </select>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- {{ t('settings.assistant.moreAgents') }}弹窗（explorer 等执行智能体的档位与模型） -->
    <Teleport to="body">
      <div
        v-if="moreModal"
        class="pointer-events-auto fixed inset-0 z-[10002] flex items-center justify-center bg-foreground/32 p-4"
        role="presentation"
        @click.self="moreModal = false"
      >
        <div class="w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
          <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">{{ t('settings.assistant.moreAgents') }}</h4>
              <p class="mt-0.5 text-[11px] text-muted">{{ t('settings.assistant.moreAgentsSheetHint') }}</p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              :aria-label="t('workspace.closeTab')"
              @click="moreModal = false"
            >
              <X class="w-4 h-4" />
            </button>
          </div>
          <div class="divide-y divide-border overflow-y-auto">
            <div
              v-for="w in moreWorkers"
              :key="'more-worker-' + w.id"
              class="px-3.5 py-3 space-y-2"
            >
              <div class="flex items-center gap-x-2 min-w-0">
                <Bot class="w-4 h-4 text-accent shrink-0" />
                <span class="text-[12px] font-medium text-foreground shrink-0">{{ composerAgentLabel(w, s.settings) }}</span>
                <span class="text-[10px] text-muted truncate min-w-0">{{ WORKER_DESC[w.id] ?? w.description }}</span>
              </div>
              <div class="flex flex-wrap items-center gap-2">
                <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                  <label
                    v-for="opt in PERFORMANCE_MODE_UI"
                    :key="'more-' + w.id + '-' + opt.value"
                    class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                    :class="(agentPerformanceModesLocal[w.id] ?? 'standard') === opt.value
                      ? 'bg-hover text-foreground'
                      : 'text-muted hover:text-foreground'"
                  >
                    <input
                      type="radio"
                      class="sr-only"
                      :name="'more-mode-' + w.id"
                      :checked="(agentPerformanceModesLocal[w.id] ?? 'standard') === opt.value"
                      @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, [w.id]: opt.value }"
                    />
                    {{ opt.label }}
                  </label>
                </div>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  :title="t('settings.assistant.configureTierModels', { name: composerAgentLabel(w, s.settings) })"
                  :aria-label="t('settings.assistant.configureTierModelsAria', { name: composerAgentLabel(w, s.settings) })"
                  @click="sceneModal = w.id"
                >
                  <SlidersHorizontal class="w-3 h-3" />
                  {{ t('settings.assistant.models') }}
                </button>
              </div>
            </div>
            <p v-if="moreWorkers.length === 0" class="text-xs text-muted text-center py-6">{{ t('settings.assistant.noMoreAgents') }}</p>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 媒体列「更多」：联网搜索工具档位（对称左侧 explore） -->
    <Teleport to="body">
      <div
        v-if="moreMediaModal"
        class="pointer-events-auto fixed inset-0 z-[10002] flex items-center justify-center bg-foreground/32 p-4"
        role="presentation"
        @click.self="moreMediaModal = false"
      >
        <div class="w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
          <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">{{ t('settings.assistant.moreTools') }}</h4>
              <p class="mt-0.5 text-[11px] text-muted">{{ t('settings.assistant.moreToolsHint') }}</p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              :aria-label="t('workspace.closeTab')"
              @click="moreMediaModal = false"
            >
              <X class="w-4 h-4" />
            </button>
          </div>
          <div class="divide-y divide-border overflow-y-auto">
            <div class="px-3.5 py-3 space-y-2">
              <div class="flex items-center gap-x-2 min-w-0">
                <Bot class="w-4 h-4 text-accent shrink-0" />
                <span class="text-[12px] font-medium text-foreground shrink-0">{{ t('settings.assistant.webSearch') }}</span>
                <span class="text-[10px] text-muted truncate min-w-0">{{ t('settings.assistant.webSearchDesc') }}</span>
              </div>
              <div class="flex flex-wrap items-center gap-2">
                <div class="inline-flex rounded-lg border border-border bg-card p-0.5">
                  <label
                    v-for="opt in PERFORMANCE_MODE_UI"
                    :key="'more-web-search-' + opt.value"
                    class="h-7 px-2.5 rounded-md text-[11px] cursor-pointer transition-colors flex items-center"
                    :class="(agentPerformanceModesLocal[WEB_SEARCH_SCENE_ID] ?? 'fast') === opt.value
                      ? 'bg-hover text-foreground'
                      : 'text-muted hover:text-foreground'"
                  >
                    <input
                      type="radio"
                      class="sr-only"
                      name="more-mode-web-search"
                      :checked="(agentPerformanceModesLocal[WEB_SEARCH_SCENE_ID] ?? 'fast') === opt.value"
                      @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, [WEB_SEARCH_SCENE_ID]: opt.value }"
                    />
                    {{ opt.label }}
                  </label>
                </div>
                <button
                  type="button"
                  class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  :title="t('settings.assistant.configureWebSearch')"
                  :aria-label="t('settings.assistant.configureWebSearchAria')"
                  @click="sceneModal = WEB_SEARCH_SCENE_ID"
                >
                  <SlidersHorizontal class="w-3 h-3" />
                  {{ t('settings.assistant.models') }}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
