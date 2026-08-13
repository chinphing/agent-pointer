<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import type { LaneQueueView, RunQueueSnapshot } from '../../../types/automation'
import { CalendarClock, ChevronRight, CircleHelp, Film, GitBranch, Monitor, Plus, ScrollText, Settings, Sparkles, Terminal, Volume2, X } from 'lucide-vue-next'
import { getDispatcherQueueSnapshot } from '../../../lib/api'
import { playTaskCompleteSound, primeTaskCompleteAudio } from '../../../lib/taskCompleteSound'
import { laneQueueLabel, shortId, triggerSourceLabel } from '../../../lib/dispatcherQueueLabels'
import { useSettingsStore } from '../../../stores/settings'

const props = defineProps<{
  form: SettingsDialogForm
}>()

const s = useSettingsStore()

const {
  TOOL_CALL_UI_FIELDS,
  displayUiChecked,
  setDisplayUi,
  computerAutoCompact,
  collapseProcessByDefault,
  taskBoardShowChildBoards,
  computerHumanLike,
  captchaSliderOffsetPx,
  computerAutoSwitchMonitor,
  mediaImageGenerationModel,
  mediaVideoGenerationModel,
  selectMediaModelWithProvider,
  maxConcurrentRuns,
  contextCompressionEnabled,
  contextBudgetTokens,
  contextKeepRecentUserTurns,
  maxToolRounds,
  parallelToolExecutionEnabled,
  maxParallelToolCalls,
  maxParallelSubAgents,
  maxParallelMediaJobs,
  mediaDeps,
  ffmpegStatusLabel,
  ffmpegStatusDetail,
  ffmpegNeedsInstall,
  refreshMediaDeps,
  askAssistantInstallFfmpeg,
  terminalEnvRows,
  addTerminalEnvRow,
  removeTerminalEnvRow,
  saveTerminalEnvRows,
  activeSection
} = props.form

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

// 仅在系统设置分区激活时轮询队列；切到其他分区暂停，避免后台空转。
watch(activeSection, section => {
  if (section === 'generation') {
    if (!queuePollTimer) {
      void refreshQueueSnapshot()
      queuePollTimer = setInterval(() => {
        void refreshQueueSnapshot()
      }, 2500)
    }
  } else if (queuePollTimer) {
    clearInterval(queuePollTimer)
    queuePollTimer = null
  }
}, { immediate: true })

const queueModalOpen = ref(false)
const mediaDepsModalOpen = ref(false)

const queueSummary = computed(() => {
  if (queueLoading.value && !queueSnapshot.value) return '加载中…'
  if (pendingRunCount.value > 0 || totalLaneWaiting.value > 0) {
    return `${pendingRunCount.value} 个待执行 · ${totalLaneWaiting.value} 个在 lane 排队`
  }
  return '当前无排队任务'
})

const soundSaving = ref(false)
const playSoundOnFinish = ref(s.userSettings.playSoundOnFinish !== false)

let terminalEnvSaveTimer: ReturnType<typeof setTimeout> | null = null
function onTerminalEnvRowChanged() {
  if (terminalEnvSaveTimer) clearTimeout(terminalEnvSaveTimer)
  terminalEnvSaveTimer = setTimeout(() => {
    terminalEnvSaveTimer = null
    saveTerminalEnvRows()
  }, 300)
}

onUnmounted(() => {
  if (queuePollTimer) clearInterval(queuePollTimer)
  if (terminalEnvSaveTimer) {
    clearTimeout(terminalEnvSaveTimer)
    saveTerminalEnvRows()
  }
})

async function onPlaySoundToggle(checked: boolean) {
  playSoundOnFinish.value = checked
  soundSaving.value = true
  try {
    await s.saveUser({ playSoundOnFinish: checked })
    console.info('[settings] playSoundOnFinish=%s', checked)
    if (checked) {
      primeTaskCompleteAudio()
      void playTaskCompleteSound()
    }
  } catch (err) {
    playSoundOnFinish.value = s.userSettings.playSoundOnFinish !== false
    console.error('[settings] failed to save playSoundOnFinish', err)
  } finally {
    soundSaving.value = false
  }
}
</script>

<template>
  <div class="flex-1 flex flex-col gap-5">
    <div>
      <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
        <Settings class="w-4 h-4 text-accent" />系统设置
      </h3>
      <p class="mt-0.5 text-xs text-muted">
        界面显示、桌面自动化与系统运行
      </p>
    </div>

    <!-- 界面显示 -->
    <section class="space-y-4" aria-labelledby="system-display-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-display-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">界面显示</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <!-- 工具调用 -->
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <h4 class="text-sm font-medium text-foreground">工具调用</h4>
        <div class="grid grid-cols-2 gap-y-3 gap-x-32">
          <div
            v-for="field in TOOL_CALL_UI_FIELDS"
            :key="field.key"
            class="flex items-center justify-between gap-3"
          >
            <h4 class="text-[12px] font-medium text-foreground">{{ field.label }}</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input type="checkbox" class="sr-only peer" :checked="displayUiChecked(field.key)" @change="setDisplayUi(field.key, ($event.target as HTMLInputElement).checked)" />
              <div class="settings-toggle-track" />
            </label>
          </div>
        </div>
      </div>

      <!-- 智能体输出 -->
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <h4 class="text-sm font-medium text-foreground">智能体输出</h4>
        <div class="grid grid-cols-2 gap-y-3 gap-x-32">
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">显示推理过程</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input type="checkbox" class="sr-only peer" :checked="displayUiChecked('showReasoning')" @change="setDisplayUi('showReasoning', ($event.target as HTMLInputElement).checked)" />
              <div class="settings-toggle-track" />
            </label>
          </div>
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">显示任务板面板</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input type="checkbox" class="sr-only peer" :checked="displayUiChecked('showTaskBoardPanel')" @change="setDisplayUi('showTaskBoardPanel', ($event.target as HTMLInputElement).checked)" />
              <div class="settings-toggle-track" />
            </label>
          </div>
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">显示子 Agent 边框面板</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input type="checkbox" class="sr-only peer" :checked="displayUiChecked('showSubAgentTrace')" @change="setDisplayUi('showSubAgentTrace', ($event.target as HTMLInputElement).checked)" />
              <div class="settings-toggle-track" />
            </label>
          </div>
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">显示子任务板</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input v-model="taskBoardShowChildBoards" type="checkbox" class="sr-only peer" />
              <div class="settings-toggle-track" />
            </label>
          </div>
        </div>
      </div>

      <!-- 执行过程 -->
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <h4 class="text-sm font-medium text-foreground">执行过程</h4>
        <div class="grid grid-cols-2 gap-y-3 gap-x-32">
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">执行时收缩为状态条</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input v-model="computerAutoCompact" type="checkbox" class="sr-only peer" />
              <div class="settings-toggle-track" />
            </label>
          </div>
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">默认收缩执行过程</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input v-model="collapseProcessByDefault" type="checkbox" class="sr-only peer" />
              <div class="settings-toggle-track" />
            </label>
          </div>
        </div>
      </div>
    </section>

    <!-- 通知 -->
    <section class="space-y-4" aria-labelledby="system-notify-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-notify-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">通知</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <p class="text-sm font-medium text-foreground flex items-center gap-2">
              <Volume2 class="w-4 h-4 text-accent" />完成时播放提示音
            </p>
            <p class="mt-1 text-sm text-muted">对话回合结束时播放短促提示音</p>
          </div>
          <label class="relative inline-flex items-center cursor-pointer shrink-0">
            <input
              type="checkbox"
              class="sr-only peer"
              :checked="playSoundOnFinish"
              :disabled="soundSaving"
              @change="onPlaySoundToggle(($event.target as HTMLInputElement).checked)"
            />
            <div class="settings-toggle-track" />
          </label>
        </div>
      </div>
    </section>

    <!-- 桌面自动化 -->
    <section class="space-y-4" aria-labelledby="system-desktop-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-desktop-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">桌面自动化</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-5">
        <div>
          <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
            <Monitor class="w-4 h-4 text-accent shrink-0" />电脑行为
          </h4>
          <p class="mt-1 text-[11px] text-muted">桌面自动化的操作行为细节；起始档位在「智能体 → 场景档位」中选择。</p>
        </div>

        <div class="border-t border-border pt-4 space-y-0 divide-y divide-border">
          <div class="flex items-start justify-between gap-4 py-3 first:pt-0">
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
    </section>

    <!-- 媒体生成 -->
    <section class="space-y-4" aria-labelledby="system-media-gen-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-media-gen-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">媒体生成</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
          <Sparkles class="w-4 h-4 text-accent" />图片 / 视频生成
        </h4>
        <p class="text-[11px] text-muted">暂时支持文本和图片生成视频。</p>
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
    </section>

    <!-- 运行环境 -->
    <section class="space-y-4" aria-labelledby="system-runtime-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-runtime-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">运行环境</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <div class="flex items-center justify-between gap-2">
          <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
            <Film class="w-4 h-4 text-accent" />多媒体理解
          </h4>
          <button
            type="button"
            class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
            @click="mediaDepsModalOpen = true"
          >
            管理
            <ChevronRight class="w-3 h-3" />
          </button>
        </div>
        <div class="flex items-center gap-2 rounded-lg border border-border bg-card/50 px-3 py-2.5">
          <span
            class="h-2 w-2 rounded-full shrink-0"
            :class="mediaDeps?.status === 'ready' ? 'bg-success' : 'bg-warning'"
            aria-hidden="true"
          />
          <span class="text-[12px] text-foreground">{{ ffmpegStatusLabel }}</span>
          <span class="ml-auto text-[11px] text-muted">ffmpeg / ffprobe</span>
        </div>
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <div class="flex items-center justify-between gap-2">
          <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
            <Terminal class="w-4 h-4 text-accent" />终端环境变量
          </h4>
          <button
            type="button"
            class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
            @click="addTerminalEnvRow(); onTerminalEnvRowChanged()"
          >
            添加
            <Plus class="w-3 h-3" />
          </button>
        </div>
        <p class="text-[11px] text-muted">Agent 终端子进程的 KEY→VALUE 环境变量覆盖（追加在进程与 .env 之后，优先级最高），保存后持久化。</p>

        <div v-if="terminalEnvRows.length === 0" class="rounded-lg border border-dashed border-border bg-card/40 px-3 py-4 text-center text-[11px] text-muted">
          暂无环境变量覆盖，点击「添加」新建
        </div>

        <div v-else class="space-y-2">
          <div
            v-for="row in terminalEnvRows"
            :key="row.id"
            class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-2 items-center"
          >
            <input
              v-model="row.key"
              type="text"
              spellcheck="false"
              placeholder="KEY（如 API_TOKEN）"
              class="h-8 px-2.5 rounded-lg bg-card border border-border text-[12px] text-foreground font-mono outline-none focus:border-accent/50 transition-colors min-w-0"
              @input="onTerminalEnvRowChanged"
            />
            <input
              v-model="row.value"
              type="text"
              spellcheck="false"
              placeholder="VALUE"
              class="h-8 px-2.5 rounded-lg bg-card border border-border text-[12px] text-foreground font-mono outline-none focus:border-accent/50 transition-colors min-w-0"
              @input="onTerminalEnvRowChanged"
            />
            <button
              type="button"
              class="p-1.5 rounded-lg text-muted hover:text-destructive hover:bg-destructive/10 cursor-pointer transition-colors shrink-0"
              :title="`删除 ${row.key || '环境变量'}`"
              aria-label="删除该环境变量"
              @click="removeTerminalEnvRow(row.id); onTerminalEnvRowChanged()"
            >
              <X class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- 任务调度 -->
    <section class="space-y-4" aria-labelledby="system-scheduler-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-scheduler-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">任务调度</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
        <div class="flex items-center gap-1.5 min-w-0">
          <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
            <CalendarClock class="w-4 h-4 text-accent" />任务调度
          </h4>
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
        <div class="flex items-center justify-between gap-2 pt-2 border-t border-border">
          <span class="text-[12px] font-medium text-foreground">队列状态</span>
          <div class="flex items-center gap-3 min-w-0">
            <span class="text-[11px] text-muted truncate">{{ queueSummary }}</span>
            <button
              type="button"
              class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
              @click="queueModalOpen = true"
            >
              查看队列
              <ChevronRight class="w-3 h-3" />
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- 并行执行与上下文压缩 -->
    <section class="space-y-4" aria-labelledby="system-exec-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-exec-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">执行与上下文</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-4">
        <div class="flex items-center justify-between gap-4">
          <div class="flex items-center gap-1.5 min-w-0">
            <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
              <GitBranch class="w-4 h-4 text-accent" />并行执行
            </h4>
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
          <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
            <ScrollText class="w-4 h-4 text-accent" />上下文自动压缩
          </h4>
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
    </section>

    <!-- 队列详情弹窗（低频） -->
    <Teleport to="body">
      <div
        v-if="queueModalOpen"
        class="pointer-events-auto fixed inset-0 z-[10001] flex items-center justify-center bg-black/55 p-4"
        role="presentation"
        @click.self="queueModalOpen = false"
      >
        <div class="w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
          <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">队列详情</h4>
              <p class="mt-0.5 text-[11px] text-muted">{{ queueSummary }}</p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              aria-label="关闭"
              @click="queueModalOpen = false"
            >
              <X class="w-4 h-4" />
            </button>
          </div>
          <div class="p-4 space-y-2 overflow-y-auto">
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
              v-if="queueSnapshot && pendingRunCount > 0"
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
              v-if="queueSnapshot && pendingRunCount === 0 && !totalLaneWaiting"
              class="text-[11px] text-muted text-center py-4"
            >
              当前无排队任务
            </p>
            <p v-else-if="!queueSnapshot && !queueLoading" class="text-[11px] text-muted text-center py-4">
              队列信息暂不可用
            </p>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 多媒体理解环境弹窗（低频） -->
    <Teleport to="body">
      <div
        v-if="mediaDepsModalOpen"
        class="pointer-events-auto fixed inset-0 z-[10001] flex items-center justify-center bg-black/55 p-4"
        role="presentation"
        @click.self="mediaDepsModalOpen = false"
      >
        <div class="w-full max-w-md rounded-xl border border-border bg-card shadow-2xl p-5 space-y-4" @click.stop>
          <div class="flex items-start justify-between gap-2">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">多媒体理解环境</h4>
              <p class="mt-0.5 text-[11px] text-muted">IM 视频与抽帧理解依赖本机 ffmpeg</p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              aria-label="关闭"
              @click="mediaDepsModalOpen = false"
            >
              <X class="w-4 h-4" />
            </button>
          </div>
          <div class="rounded-lg border border-border bg-card/50 px-3 py-2.5 space-y-1">
            <p class="text-[12px] font-medium text-foreground">ffmpeg / ffprobe</p>
            <p class="text-[11px] text-muted">IM 视频与抽帧理解需要本机安装；未安装时不打包进应用。</p>
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
          <div class="flex items-center justify-end gap-2">
            <button
              type="button"
              class="h-8 px-3 rounded-lg border border-border text-xs text-foreground hover:bg-muted/50 cursor-pointer transition-colors"
              @click="refreshMediaDeps()"
            >
              重新检测
            </button>
            <button
              v-if="ffmpegNeedsInstall"
              type="button"
              class="h-8 px-3 rounded-lg bg-accent text-accent-foreground text-xs hover:opacity-90 cursor-pointer transition-colors"
              @click="askAssistantInstallFfmpeg()"
            >
              让助手安装
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
