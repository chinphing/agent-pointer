<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { COMPUTER_INITIAL_TIER_OPTIONS } from '../../../types/chat'
import type { LaneQueueView, RunQueueSnapshot } from '../../../types/automation'
import { useSettingsStore } from '../../../stores/settings'
import { getDispatcherQueueSnapshot } from '../../../lib/api'
import { laneQueueLabel, shortId, triggerSourceLabel } from '../../../lib/dispatcherQueueLabels'
import { Bot, CircleHelp, Monitor, Sparkles, Wrench } from 'lucide-vue-next'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const s = useSettingsStore()
const {
  PERFORMANCE_MODE_HELP,
  AGENT_MODE_USER_ROWS,
  MEDIA_MODE_USER_ROWS,
  PERFORMANCE_MODE_UI,
  computerInitialTier,
  agentPerformanceModesLocal,
  mediaUnderstandingModesLocal,
  computerAutoCompact,
  collapseProcessByDefault,
  userCodingRules,
  computerHumanLike,
  captchaSliderOffsetPx,
  computerAutoSwitchMonitor,
  mediaImageGenerationModel,
  mediaVideoGenerationModel,
  selectMediaModelWithProvider,
  toolApprovalMode,
  contextCompressionEnabled,
  contextBudgetTokens,
  contextKeepRecentUserTurns,
  maxToolRounds,
  parallelToolExecutionEnabled,
  maxParallelToolCalls,
  maxParallelSubAgents,
  maxParallelMediaJobs,
  maxConcurrentRuns,
  mediaDeps,
  ffmpegStatusLabel,
  ffmpegStatusDetail,
  ffmpegNeedsInstall,
  refreshMediaDeps,
  askAssistantInstallFfmpeg,
  activeSection
} = props.form

function openModelService() {
  activeSection.value = 'provider'
}

const queueSnapshot = ref<RunQueueSnapshot | null>(null)
const queueLoading = ref(false)
let queuePollTimer: ReturnType<typeof setInterval> | null = null

const activeLanes = computed(() =>
  (queueSnapshot.value?.lanes ?? []).filter(
    lane => lane.lane.startsWith('session:')
      ? lane.active > 0 || lane.waiting > 0
      : true
  )
)

const pendingRunCount = computed(() => queueSnapshot.value?.pendingRuns.length ?? 0)

const totalLaneWaiting = computed(() =>
  activeLanes.value.reduce((sum, lane) => sum + lane.waiting, 0)
)

function laneStatusLine(lane: LaneQueueView): string {
  return `${lane.active}/${lane.maxConcurrent} 执行中 · ${lane.waiting} 排队`
}

async function refreshQueueSnapshot() {
  queueLoading.value = true
  try {
    queueSnapshot.value = await getDispatcherQueueSnapshot()
  } catch (e) {
    console.warn('[settings] getDispatcherQueueSnapshot failed', e)
  } finally {
    queueLoading.value = false
  }
}

onMounted(() => {
  void refreshQueueSnapshot()
  queuePollTimer = setInterval(() => {
    void refreshQueueSnapshot()
  }, 2500)
})

onUnmounted(() => {
  if (queuePollTimer) clearInterval(queuePollTimer)
})

const COMPUTER_TIER_DESCRIPTIONS = {
  primary: '轻量视觉，响应更快',
  intermediate: '速度与准确度平衡',
  advanced: '最强视觉，适合复杂界面'
} as const

const COMPUTER_TIER_CARDS = COMPUTER_INITIAL_TIER_OPTIONS.map(option => ({
  ...option,
  desc: COMPUTER_TIER_DESCRIPTIONS[option.value]
}))
</script>

<template>            <div>
              <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
                <Bot class="w-4 h-4 text-accent" />智能体
              </h3>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
              <div>
                <div class="flex items-center gap-1.5">
                  <h4 class="text-sm font-medium text-foreground">模式选择</h4>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
                    :title="PERFORMANCE_MODE_HELP"
                    aria-label="模式说明"
                    @click.stop
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <p class="mt-1 text-[11px] text-muted">
                  选择各场景使用的档位；每档具体模型在「模型服务」中配置。
                  <button type="button" class="ml-1 text-accent hover:underline cursor-pointer" @click="openModelService">去配置</button>
                </p>
              </div>
              <div class="grid grid-cols-1 lg:grid-cols-2 gap-6 lg:gap-8">
                <div class="space-y-3 min-w-0">
                  <h5 class="text-[12px] font-medium text-foreground flex items-center gap-1.5">
                    <Bot class="w-3.5 h-3.5 text-accent shrink-0" />智能体
                  </h5>
                  <div
                    v-for="row in AGENT_MODE_USER_ROWS"
                    :key="'agent-mode-row-' + row.id"
                    class="flex flex-wrap items-center gap-x-4 gap-y-2"
                  >
                    <span class="text-[12px] text-foreground whitespace-nowrap shrink-0 w-20">{{ row.label }}</span>
                    <div class="grid grid-cols-3 gap-1.5 min-w-0 flex-1 max-w-sm">
                      <label
                        v-for="opt in PERFORMANCE_MODE_UI"
                        :key="row.id + '-mode-' + opt.value"
                        class="rounded-lg border px-2 py-1.5 text-center cursor-pointer transition-all"
                        :class="(agentPerformanceModesLocal[row.id] ?? 'fast') === opt.value
                          ? 'border-accent/40 bg-accent/5 text-foreground'
                          : 'border-border bg-card text-muted hover:border-border/80'"
                      >
                        <input
                          type="radio"
                          class="sr-only"
                          :name="'agent-mode-' + row.id"
                          :checked="(agentPerformanceModesLocal[row.id] ?? 'fast') === opt.value"
                          @change="agentPerformanceModesLocal = { ...agentPerformanceModesLocal, [row.id]: opt.value }"
                        />
                        <span class="text-[11px] whitespace-nowrap">{{ opt.label }}</span>
                      </label>
                    </div>
                  </div>
                </div>
                <div class="space-y-3 min-w-0 lg:border-l lg:border-border lg:pl-8">
                  <h5 class="text-[12px] font-medium text-foreground flex items-center gap-1.5">
                    <Wrench class="w-3.5 h-3.5 text-accent shrink-0" />工具
                  </h5>
                  <div
                    v-for="row in MEDIA_MODE_USER_ROWS"
                    :key="'media-mode-row-' + row.key"
                    class="flex flex-wrap items-center gap-x-4 gap-y-2"
                  >
                    <span class="text-[12px] text-foreground whitespace-nowrap shrink-0 w-20">{{ row.label }}</span>
                    <div class="grid grid-cols-3 gap-1.5 min-w-0 flex-1 max-w-sm">
                      <label
                        v-for="opt in PERFORMANCE_MODE_UI"
                        :key="row.key + '-mode-' + opt.value"
                        class="rounded-lg border px-2 py-1.5 text-center cursor-pointer transition-all"
                        :class="mediaUnderstandingModesLocal[row.key] === opt.value
                          ? 'border-accent/40 bg-accent/5 text-foreground'
                          : 'border-border bg-card text-muted hover:border-border/80'"
                      >
                        <input
                          type="radio"
                          class="sr-only"
                          :name="'media-mode-' + row.key"
                          :checked="mediaUnderstandingModesLocal[row.key] === opt.value"
                          @change="mediaUnderstandingModesLocal = { ...mediaUnderstandingModesLocal, [row.key]: opt.value }"
                        />
                        <span class="text-[11px] whitespace-nowrap">{{ opt.label }}</span>
                      </label>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-5">
              <div>
                <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
                  <Monitor class="w-4 h-4 text-accent shrink-0" />电脑操控
                </h4>
                <p class="mt-1 text-[11px] text-muted">
                  桌面自动化的起始视觉档位与操作行为；每档具体模型在「模型服务」中配置。
                  <button type="button" class="ml-1 text-accent hover:underline cursor-pointer" @click="openModelService">去配置</button>
                </p>
              </div>

              <div class="space-y-2.5">
                <p class="text-[12px] font-medium text-foreground">起始模式</p>
                <div class="grid grid-cols-1 sm:grid-cols-3 gap-2">
                  <label
                    v-for="opt in COMPUTER_TIER_CARDS"
                    :key="'computer-tier-card-' + opt.value"
                    class="rounded-xl border p-3 cursor-pointer transition-all"
                    :class="
                      computerInitialTier === opt.value
                        ? 'border-accent/40 bg-accent/5'
                        : 'border-border bg-card hover:border-border/80'
                    "
                  >
                    <input
                      v-model="computerInitialTier"
                      type="radio"
                      class="sr-only"
                      name="computer-initial-tier"
                      :value="opt.value"
                    />
                    <span class="block text-sm font-medium text-foreground">{{ opt.label }}</span>
                    <span class="mt-1 block text-[11px] text-muted leading-snug">{{ opt.desc }}</span>
                  </label>
                </div>
                <p class="text-[10px] text-muted">仅影响新会话；验证失败时可能自动升档。</p>
              </div>

              <div class="border-t border-border pt-4 space-y-0 divide-y divide-border">
                <div class="flex items-start justify-between gap-4 py-3 first:pt-0">
                  <div class="min-w-0">
                    <p class="text-[12px] font-medium text-foreground">执行时收缩为状态条</p>
                    <p class="text-[11px] text-muted mt-0.5">运行中收起对话区域，保留进度提示</p>
                  </div>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0 mt-0.5">
                    <input
                      type="checkbox"
                      class="sr-only peer"
                      :checked="computerAutoCompact"
                      @change="computerAutoCompact = ($event.target as HTMLInputElement).checked"
                    />
                    <div class="settings-toggle-track" />
                  </label>
                </div>

                <div class="flex items-start justify-between gap-4 py-3">
                  <div class="min-w-0">
                    <p class="text-[12px] font-medium text-foreground">默认收缩执行过程</p>
                    <p class="text-[11px] text-muted mt-0.5">已完成轮次始终折叠，仅显示用户问题和最终输出</p>
                  </div>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0 mt-0.5">
                    <input
                      type="checkbox"
                      class="sr-only peer"
                      :checked="collapseProcessByDefault"
                      @change="collapseProcessByDefault = ($event.target as HTMLInputElement).checked"
                    />
                    <div class="settings-toggle-track" />
                  </label>
                </div>

                <div class="flex items-start justify-between gap-4 py-3">
                  <div class="min-w-0">
                    <p class="text-[12px] font-medium text-foreground">人性化鼠标移动</p>
                    <p class="text-[11px] text-muted mt-0.5">曲线轨迹与微抖动；关闭时为直线匀速移动</p>
                  </div>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0 mt-0.5">
                    <input
                      type="checkbox"
                      class="sr-only peer"
                      :checked="computerHumanLike"
                      @change="computerHumanLike = ($event.target as HTMLInputElement).checked"
                    />
                    <div class="settings-toggle-track" />
                  </label>
                </div>

                <div class="flex items-start justify-between gap-4 py-3">
                  <div class="min-w-0">
                    <p class="text-[12px] font-medium text-foreground">自动切换屏幕</p>
                    <p class="text-[11px] text-muted mt-0.5">默认主屏，打开应用后跟随窗口所在显示器</p>
                  </div>
                  <label class="relative inline-flex items-center cursor-pointer shrink-0 mt-0.5">
                    <input
                      type="checkbox"
                      class="sr-only peer"
                      :checked="computerAutoSwitchMonitor"
                      @change="computerAutoSwitchMonitor = ($event.target as HTMLInputElement).checked"
                    />
                    <div class="settings-toggle-track" />
                  </label>
                </div>

                <div class="flex items-start justify-between gap-4 py-3">
                  <div class="min-w-0">
                    <p class="text-[12px] font-medium text-foreground">滑块验证偏移</p>
                    <p class="text-[11px] text-muted mt-0.5">滑块验证码拖拽终点的像素微调</p>
                  </div>
                  <input
                    v-model.number="captchaSliderOffsetPx"
                    type="number"
                    step="1"
                    class="w-20 h-9 shrink-0 rounded-lg border border-border bg-card px-2 text-[12px] text-right text-foreground outline-none focus:border-accent/50 mt-0.5"
                  />
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
                <Sparkles class="w-4 h-4 text-accent" />图片 / 视频生成
              </h4>
              <p class="text-[11px] text-muted">
                暂时支持文本和图片生成视频。
              </p>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">图片生成</label>
                  <select
                    :value="mediaImageGenerationModel"
                    class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground cursor-pointer outline-none focus:border-accent/50"
                    @change="selectMediaModelWithProvider('imageGeneration', ($event.target as HTMLSelectElement).value)"
                  >
                    <option value="">默认（千问 wan2.7-image-pro 或豆包 Seedream）</option>
                    <option
                      v-for="item in s.imageGenerationModels"
                      :key="'img-gen-' + item.providerId + ':' + item.model"
                      :value="item.providerId + ':' + item.model"
                    >
                      {{ item.providerName }} / {{ item.model }}
                    </option>
                  </select>
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">视频生成</label>
                  <select
                    :value="mediaVideoGenerationModel"
                    class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground cursor-pointer outline-none focus:border-accent/50"
                    @change="selectMediaModelWithProvider('videoGeneration', ($event.target as HTMLSelectElement).value)"
                  >
                    <option value="">默认（千问 HappyHorse 或豆包 Seedance 2.0）</option>
                    <option
                      v-for="item in s.videoGenerationModels"
                      :key="'vid-gen-' + item.providerId + ':' + item.model"
                      :value="item.providerId + ':' + item.model"
                    >
                      {{ item.providerName }} / {{ item.model }}
                    </option>
                  </select>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
                <Wrench class="w-4 h-4 text-accent" />工具使用权限
              </h4>
              <div class="grid grid-cols-2 gap-3">
                <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'auto' ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'">
                  <input v-model="toolApprovalMode" type="radio" value="auto" class="sr-only" />
                  <span class="block text-sm text-foreground">自动执行</span>
                  <span class="mt-1 block text-[11px] text-muted">AI 使用工具时自动执行，无需确认</span>
                </label>
                <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'manual' ? 'border-accent/40 bg-accent/5' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'">
                  <input v-model="toolApprovalMode" type="radio" value="manual" class="sr-only" />
                  <span class="block text-sm text-foreground">敏感操作确认</span>
                  <span class="mt-1 block text-[11px] text-muted">涉及文件、命令等操作时需要你确认</span>
                </label>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <div>
                <h4 class="text-sm font-medium text-foreground">个性化</h4>
                <p class="mt-1 text-[11px] text-muted">
                  写入每次对话的系统提示（[USER RULES]）。用于约束改动范围、风格等；留空则仅使用产品默认规则。
                </p>
              </div>
              <textarea
                v-model="userCodingRules"
                rows="5"
                maxlength="4000"
                placeholder="例如：&#10;- 只改用户明确要求的行为，不顺手重构&#10;- 歧义时先说明假设，不要扩大修复范围&#10;- 相关但未要求的内容放到可选后续，不要一并提交"
                class="w-full min-h-[7rem] px-3 py-2 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors resize-y"
              />
              <p class="text-[10px] text-muted text-right">{{ userCodingRules.length }} / 4000</p>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
              <div class="flex items-center gap-1.5 min-w-0">
                <h4 class="text-sm font-medium text-foreground">任务调度</h4>
                <button
                  type="button"
                  class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
                  title="同时执行的 Agent 运行数上限，聊天、Webhook、Cron 等触发源共享此配额。"
                  aria-label="任务调度说明"
                >
                  <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                </button>
              </div>
              <div class="max-w-xs">
                <label
                  class="block text-[12px] text-muted mb-1.5"
                  title="不同会话可并行运行，同一会话仍串行"
                >全局并发任务</label>
                <input
                  v-model.number="maxConcurrentRuns"
                  type="number"
                  min="1"
                  max="64"
                  step="1"
                  class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>

              <div class="pt-2 border-t border-border space-y-3">
                <div class="flex items-center justify-between gap-2">
                  <span class="text-[12px] font-medium text-foreground">队列状态</span>
                  <span class="text-[11px] text-muted">
                    <template v-if="queueLoading && !queueSnapshot">加载中…</template>
                    <template v-else>
                      {{ pendingRunCount }} 个待执行
                      <template v-if="totalLaneWaiting > 0"> · {{ totalLaneWaiting }} 个在 lane 排队</template>
                    </template>
                  </span>
                </div>

                <div v-if="queueSnapshot" class="space-y-2">
                  <div
                    v-for="lane in activeLanes"
                    :key="lane.lane"
                    class="rounded-lg border border-border bg-[hsl(var(--card-elevated))] px-3 py-2"
                  >
                    <div class="flex items-center justify-between gap-2 text-[12px]">
                      <span class="font-medium text-foreground">{{ laneQueueLabel(lane.lane) }}</span>
                      <span class="text-muted shrink-0">{{ laneStatusLine(lane) }}</span>
                    </div>
                    <ul v-if="lane.waiters.length" class="mt-2 space-y-1">
                      <li
                        v-for="w in lane.waiters"
                        :key="`${lane.lane}:${w.runId}`"
                        class="text-[11px] text-muted flex items-center gap-1.5 min-w-0"
                        :title="`${w.runId} · ${w.conversationId}`"
                      >
                        <span class="shrink-0 rounded px-1 py-0.5 bg-accent-muted text-accent text-[10px]">{{ triggerSourceLabel(w.triggerSource) }}</span>
                        <span class="truncate">{{ shortId(w.conversationId, 28) }}</span>
                      </li>
                    </ul>
                  </div>

                  <div
                    v-if="pendingRunCount > 0"
                    class="rounded-lg border border-border bg-[hsl(var(--card-elevated))] px-3 py-2 space-y-1.5"
                  >
                    <div class="text-[12px] font-medium text-foreground">待执行任务</div>
                    <ul class="space-y-1 max-h-36 overflow-y-auto">
                      <li
                        v-for="run in queueSnapshot.pendingRuns"
                        :key="run.runId"
                        class="text-[11px] text-muted flex items-center gap-1.5 min-w-0"
                        :title="run.runId"
                      >
                        <span class="shrink-0 rounded px-1 py-0.5 bg-accent-muted text-accent text-[10px]">{{ triggerSourceLabel(run.triggerSource) }}</span>
                        <span class="truncate flex-1">{{ shortId(run.conversationId, 24) }}</span>
                        <span class="shrink-0 text-[10px] text-muted/70">{{ new Date(run.createdAtMs).toLocaleTimeString() }}</span>
                      </li>
                    </ul>
                  </div>

                  <p
                    v-else-if="!totalLaneWaiting"
                    class="text-[11px] text-muted text-center py-2"
                  >
                    当前无排队任务
                  </p>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
              <div class="flex items-center justify-between gap-4">
                <div class="flex items-center gap-1.5 min-w-0">
                  <h4 class="text-sm font-medium text-foreground">并行执行</h4>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors shrink-0"
                    title="同一轮多个工具调用时，无冲突的可并行；关闭后全部串行。并发上限留空时按 CPU 核数，最多 8。"
                    aria-label="并行执行说明"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <label class="relative inline-flex items-center cursor-pointer shrink-0">
                  <input v-model="parallelToolExecutionEnabled" type="checkbox" class="sr-only peer" />
                  <div class="settings-toggle-track"></div>
                </label>
              </div>

              <div
                v-if="parallelToolExecutionEnabled"
                class="grid grid-cols-1 sm:grid-cols-3 gap-3 pt-2 border-t border-border"
              >
                  <div>
                    <label
                      class="block text-[12px] text-muted mb-1.5"
                      title="文件、终端、搜索等通用工具；留空时按 CPU 核数，上限 8"
                    >通用上限</label>
                    <input
                      v-model="maxParallelToolCalls"
                      type="number"
                      min="1"
                      max="64"
                      step="1"
                      placeholder="自动"
                      class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors placeholder:text-muted/60"
                    />
                  </div>
                  <div>
                    <label
                      class="block text-[12px] text-muted mb-1.5"
                      title="run_subagent 并发；留空时与通用上限相同"
                    >子 Agent</label>
                    <input
                      v-model="maxParallelSubAgents"
                      type="number"
                      min="1"
                      max="64"
                      step="1"
                      placeholder="自动"
                      class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors placeholder:text-muted/60"
                    />
                  </div>
                  <div>
                    <label
                      class="block text-[12px] text-muted mb-1.5"
                      title="图片 / 视频生成与 media_understand"
                    >媒体任务</label>
                    <input
                      v-model="maxParallelMediaJobs"
                      type="number"
                      min="1"
                      max="64"
                      step="1"
                      placeholder="自动"
                      class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors placeholder:text-muted/60"
                    />
                  </div>
                </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
              <div class="flex items-center justify-between">
                <h4 class="text-sm font-medium text-foreground">上下文自动压缩</h4>
                <label class="relative inline-flex items-center cursor-pointer">
                  <input v-model="contextCompressionEnabled" type="checkbox" class="sr-only peer" />
                  <div class="settings-toggle-track"></div>
                </label>
              </div>
              <p class="text-[11px] text-muted">当历史消息超过预算时，自动生成摘要并保留最近若干轮对话原文。</p>

              <div v-if="contextCompressionEnabled" class="grid grid-cols-3 gap-3 pt-2 border-t border-border">
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">触发预算（tokens）</label>
                  <input v-model.number="contextBudgetTokens" type="number" min="4096" max="2000000" step="1000" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">保留最近用户轮数</label>
                  <input v-model.number="contextKeepRecentUserTurns" type="number" min="1" max="50" step="1" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
                <div>
                  <label class="block text-[12px] text-muted mb-1.5">单轮最大工具调用轮次</label>
                  <input v-model.number="maxToolRounds" type="number" min="1" max="10000" step="1" class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground outline-none focus:border-accent/50 transition-colors" />
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
              <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
                <Sparkles class="w-4 h-4 text-accent" />多媒体理解
              </h4>
              <div class="rounded-lg border border-border bg-card/50 px-3 py-2.5 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
                <div>
                  <p class="text-[12px] text-foreground">ffmpeg / ffprobe</p>
                  <p class="text-[11px] text-muted">
                    IM 视频与抽帧理解需要本机安装；未安装时不打包进应用。
                  </p>
                  <p
                    class="text-[11px] mt-1"
                    :class="mediaDeps?.status === 'ready' ? 'text-emerald-600' : 'text-amber-600'"
                  >
                    {{ ffmpegStatusLabel }}
                  </p>
                  <p v-if="ffmpegStatusDetail" class="text-[10px] text-muted mt-0.5 break-all">
                    {{ ffmpegStatusDetail }}
                  </p>
                  <p v-if="mediaDeps?.status === 'ready'" class="text-[10px] text-muted mt-0.5">
                    单个视频仍可能因编码或文件损坏抽帧失败，不代表未安装 ffmpeg。
                  </p>
                </div>
                <div class="flex items-center gap-2 shrink-0">
                  <button
                    type="button"
                    class="h-8 px-3 rounded-lg border border-border text-xs text-foreground hover:bg-muted/50"
                    @click="refreshMediaDeps()"
                  >
                    重新检测
                  </button>
                  <button
                    v-if="ffmpegNeedsInstall"
                    type="button"
                    class="h-8 px-3 rounded-lg bg-accent text-accent-foreground text-xs hover:opacity-90"
                    @click="askAssistantInstallFfmpeg()"
                  >
                    让助手安装
                  </button>
                </div>
              </div>
            </div>
</template>
