<script setup lang="ts">
import { useI18n } from 'vue-i18n'
const { t } = useI18n()

import { computed, ref } from 'vue'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { useSettingsStore } from '../../../stores/settings'
import { CircleHelp, Code, Film, Gauge, Monitor, SlidersHorizontal, UserRound, Bot, X } from 'lucide-vue-next'
import { composerAgentLabel } from '../../../lib/agentUi'
import SceneTierModelsModal from '../SceneTierModelsModal.vue'
import { thinkingIntensityOptions, patchTierThinkingIntensity, tierThinkingIntensityValue } from '../../../lib/thinkingIntensity'
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

type PipelineSlot = 'locate' | 'locateVerify' | 'verify'

/**
 * 三个槽位各自按档位配置 —— 与「三档模型映射」里主模型的三档是**两套独立配置**，
 * 互不影响：主模型配的是 computer 自己用哪个模型，这里配的是定位链各阶段用哪个。
 */
const PIPELINE_SLOTS = computed<{ kind: PipelineSlot; label: string; hint: string }[]>(() => [
  {
    kind: 'locate',
    label: t('settings.agentPanel.locateName'),
    hint: t('settings.agentPanel.locateHint')
  },
  {
    kind: 'locateVerify',
    label: t('settings.agentPanel.locateVerifyName'),
    hint: t('settings.agentPanel.locateVerifyHint')
  },
  {
    kind: 'verify',
    label: t('settings.agentPanel.verifyName'),
    hint: t('settings.agentPanel.verifyHint')
  }
])

/** 「继承」项文案：带上该档位实际会继承到的全局值。 */
function inheritLabel(kind: PipelineSlot, mode: string): string {
  const base = t('settings.agentPanel.inheritGlobal')
  const v = props.form.inheritedPipelineLabel(kind, mode)
  return v ? `${base}（${v}）` : base
}

// 媒体列「更多」：联网搜索等工具档位（非子智能体，与左侧 explore 入口对称）
const moreMediaModal = ref(false)
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
  computerPipelineLlm,
  pipelineTierIntensity,
  pipelineTierValue,
  selectPipelineTierModel,
  setPipelineTierIntensity,
  enabledWorkers,
  agentPerformanceModesLocal,
  mediaUnderstanding,
  selectMediaUnderstandingModel,
  patchMediaUnderstandingModel,
  webSearchLlm,
  selectWebSearchModel,
  patchWebSearchLlm,
  userCodingRules
} = props.form

/** 媒体理解的候选模型：音频走转写模型，图片/视频走视觉模型。 */
function mediaOptions(kind: 'image' | 'audio' | 'video') {
  return kind === 'audio' ? s.audioModels : s.visionModels
}

/** 思考强度：'' 表示跟随模型。 */
const MEDIA_THINKING_OPTIONS = computed(() => thinkingIntensityOptions())

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
              <select
                class="h-8 min-w-0 flex-1 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                :value="mediaUnderstanding(row.key).providerId + ':' + mediaUnderstanding(row.key).model"
                @change="selectMediaUnderstandingModel(row.key, ($event.target as HTMLSelectElement).value)"
              >
                <option value="">{{ t('settings.thinking.unset') }}</option>
                <option
                  v-for="item in mediaOptions(row.key)"
                  :key="item.providerId + ':' + item.model"
                  :value="item.providerId + ':' + item.model"
                >{{ item.providerName }} / {{ item.model }}</option>
              </select>
              <select
                class="h-8 w-[6.5rem] px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                :value="tierThinkingIntensityValue(mediaUnderstanding(row.key))"
                :title="t('settings.agentPanel.thinkingIntensity')"
                @change="patchMediaUnderstandingModel(row.key, patchTierThinkingIntensity(($event.target as HTMLSelectElement).value as '' | 'off' | 'low' | 'medium' | 'high' | 'max'))"
              >
                <option v-for="opt in MEDIA_THINKING_OPTIONS" :key="opt.value || 'unset'" :value="opt.value">{{ opt.label }}</option>
              </select>
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
        <div class="w-[23rem] max-w-[92vw] max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
          <div class="flex items-start justify-between gap-2 border-b border-border px-4 py-3 shrink-0">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">{{ t('settings.assistant.computerAdvancedTitle') }}</h4>
              <p class="mt-0.5 text-[11px] text-muted">{{ t('settings.assistant.computerAdvancedHint') }}</p>
              <p class="mt-1 text-[11px] font-medium text-accent">
                {{ t('settings.agentPanel.pipelineTierHint') }}
              </p>
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
            <div
              v-for="slot in PIPELINE_SLOTS"
              :key="'pipeline-' + slot.kind"
              class="px-3 py-2.5 space-y-1.5"
            >
              <div class="flex items-center gap-x-1.5">
                <Monitor class="w-3.5 h-3.5 text-accent shrink-0" />
                <span class="text-[12px] font-medium text-foreground">{{ slot.label }}</span>
              </div>
              <p class="text-[10px] text-muted leading-snug">{{ slot.hint }}</p>
              <div class="space-y-1">
                <div
                  v-for="tier in PERFORMANCE_MODE_UI"
                  :key="slot.kind + '-' + tier.value"
                  class="grid grid-cols-[2.2rem_minmax(0,1fr)_4.5rem] items-center gap-1.5"
                >
                  <span class="text-[10px] text-muted">{{ tier.label }}</span>
                  <select
                    :value="pipelineTierValue(slot.kind, tier.value)"
                    class="h-7 min-w-0 px-1.5 rounded border border-border bg-[hsl(var(--card-elevated))] text-[11px] text-foreground outline-none focus:border-accent/50"
                    @change="selectPipelineTierModel(slot.kind, tier.value, ($event.target as HTMLSelectElement).value)"
                  >
                    <option value="">{{ inheritLabel(slot.kind, tier.value) }}</option>
                    <option
                      v-for="item in s.allModels"
                      :key="slot.kind + tier.value + item.providerId + ':' + item.model"
                      :value="item.providerId + ':' + item.model"
                    >{{ item.providerName }} / {{ item.model }}</option>
                  </select>
                  <select
                    :value="pipelineTierIntensity(slot.kind, tier.value)"
                    class="h-7 px-1 rounded border border-border bg-[hsl(var(--card-elevated))] text-[11px] text-foreground outline-none focus:border-accent/50"
                    @change="setPipelineTierIntensity(slot.kind, tier.value, ($event.target as HTMLSelectElement).value)"
                  >
                    <option value="">{{ t('settings.agentPanel.inheritGlobal') }}</option>
                    <option
                      v-for="opt in THINKING_OPTIONS"
                      :key="slot.kind + tier.value + '-t-' + opt.value"
                      :value="opt.value"
                    >{{ opt.label }}</option>
                  </select>
                </div>
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
                <select
                  class="h-8 min-w-0 flex-1 px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                  :value="webSearchLlm().providerId + ':' + webSearchLlm().model"
                  @change="selectWebSearchModel(($event.target as HTMLSelectElement).value)"
                >
                  <option value="">{{ t('settings.thinking.unset') }}</option>
                  <option
                    v-for="item in s.allModels"
                    :key="'web-search-' + item.providerId + ':' + item.model"
                    :value="item.providerId + ':' + item.model"
                  >{{ item.providerName }} / {{ item.model }}</option>
                </select>
                <select
                  class="h-8 w-[6.5rem] px-2 rounded border border-border bg-card text-[11px] text-foreground outline-none focus:border-accent/50"
                  :value="tierThinkingIntensityValue(webSearchLlm())"
                  :title="t('settings.agentPanel.thinkingIntensity')"
                  @change="patchWebSearchLlm(patchTierThinkingIntensity(($event.target as HTMLSelectElement).value as '' | 'off' | 'low' | 'medium' | 'high' | 'max'))"
                >
                  <option v-for="opt in MEDIA_THINKING_OPTIONS" :key="'ws-' + (opt.value || 'unset')" :value="opt.value">{{ opt.label }}</option>
                </select>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
