<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import type { LaneQueueView, RunQueueSnapshot } from '../../../types/automation'
import type { UiLocalePreference } from '../../../types/chat'
import { ChevronRight, CircleHelp, Film, Languages, Monitor, Plus, Sparkles, Terminal, Volume2, Wrench, X } from 'lucide-vue-next'
import { getDispatcherQueueSnapshot } from '../../../lib/api'
import { playTaskCompleteSound, primeTaskCompleteAudio } from '../../../lib/taskCompleteSound'
import { laneQueueLabel, shortId, triggerSourceLabel } from '../../../lib/dispatcherQueueLabels'
import { isSameTierRef, platformMediaGenerationDefault } from '../../../lib/platformTierDefaults'
import { applyUiLocale, normalizeUiLocalePreference } from '../../../lib/uiLocale'
import { useSettingsStore } from '../../../stores/settings'

const { t } = useI18n()

const props = defineProps<{
  form: SettingsDialogForm
}>()

const s = useSettingsStore()

function mediaGenDefaultLabel(kind: 'image' | 'video'): string {
  const ref = platformMediaGenerationDefault(s.platformSettings.tierDefaults, kind)
  return ref?.model
    ? t('settings.platformDefaultWithModel', { model: ref.model })
    : t('settings.platformDefault')
}

function mediaGenOverridden(kind: 'imageGeneration' | 'videoGeneration', platformKind: 'image' | 'video'): boolean {
  const userRef = s.userSettings.mediaModelOverrides?.[kind]
  if (!userRef?.model?.trim()) return false
  const platformRef = platformMediaGenerationDefault(s.platformSettings.tierDefaults, platformKind)
  return !isSameTierRef(userRef, platformRef)
}

const imageGenDefaultLabel = computed(() => mediaGenDefaultLabel('image'))
const videoGenDefaultLabel = computed(() => mediaGenDefaultLabel('video'))
const imageGenOverridden = computed(() => mediaGenOverridden('imageGeneration', 'image'))
const videoGenOverridden = computed(() => mediaGenOverridden('videoGeneration', 'video'))

const {
  TOOL_CALL_UI_FIELDS,
  displayUiChecked,
  setDisplayUi,
  computerAutoCompact,
  collapseProcessByDefault,
  taskBoardShowChildBoards,
  computerHumanLike,
  computerAutoSwitchMonitor,
  mediaImageGenerationModel,
  mediaVideoGenerationModel,
  selectMediaModelWithProvider,
  maxToolRounds,
  maxSubAgentToolRounds,
  maxConcurrentRuns,
  fileReadMaxKb,
  fileLineMaxBytes,
  fileGrepMaxResults,
  terminalOutputMaxKb,
  terminalTimeoutSeconds,
  terminalMaxWallHours,
  attachmentUploadMaxMb,
  parallelToolExecutionEnabled,
  maxParallelToolCalls,
  maxParallelSubAgents,
  maxParallelMediaJobs,
  commitMaxParallelToolCalls,
  commitMaxParallelSubAgents,
  commitMaxParallelMediaJobs,
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
  activeSection,
  toolApprovalMode
} = props.form

function restoreTimeoutSeconds(value: unknown): number {
  const n = Number(value)
  if (!Number.isFinite(n) || n < 1) return 30
  return Math.min(86_400, Math.floor(n))
}

function restoreWallHours(value: unknown): number {
  const n = Number(value)
  if (!Number.isFinite(n) || n < 1) return 24
  return Math.min(10_000, Math.floor(n))
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
  return t('settings.queue.laneActive', { active: lane.active, max: lane.maxConcurrent, waiting: lane.waiting })
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

const queueBusy = computed(() => pendingRunCount.value > 0 || totalLaneWaiting.value > 0)

const queueStatusTitle = computed(() => {
  if (queueLoading.value && !queueSnapshot.value) return t('settings.queue.loading')
  if (queueBusy.value) {
    return t('settings.queue.summary', { pending: pendingRunCount.value, waiting: totalLaneWaiting.value })
  }
  return t('settings.queue.empty')
})

const soundSaving = ref(false)
const playSoundOnFinish = ref(s.userSettings.playSoundOnFinish !== false)

const localeSaving = ref(false)
const uiLocale = ref<UiLocalePreference>(
  normalizeUiLocalePreference(s.userSettings.uiLocale) as UiLocalePreference
)

watch(
  () => s.userSettings.uiLocale,
  v => {
    uiLocale.value = normalizeUiLocalePreference(v) as UiLocalePreference
  }
)

const localeOptions: { value: UiLocalePreference; labelKey: string }[] = [
  { value: 'system', labelKey: 'settings.languageSystem' },
  { value: 'zh-CN', labelKey: 'settings.languageZhCN' },
  { value: 'en', labelKey: 'settings.languageEn' }
]

async function onUiLocaleChange(next: UiLocalePreference) {
  const prev = uiLocale.value
  uiLocale.value = next
  applyUiLocale(next)
  localeSaving.value = true
  try {
    await s.saveUser({ uiLocale: next })
    console.info('[settings] uiLocale=%s', next)
  } catch (err) {
    uiLocale.value = prev
    applyUiLocale(prev)
    console.error('[settings] failed to save uiLocale', err)
  } finally {
    localeSaving.value = false
  }
}

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
    <!-- 界面显示 -->
    <section class="space-y-4" aria-labelledby="system-display-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-display-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">{{ t('settings.display.heading') }}</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <!-- Language -->
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <p class="text-sm font-medium text-foreground flex items-center gap-2">
              <Languages class="w-4 h-4 text-accent" />{{ t('settings.language') }}
            </p>
            <p class="mt-1 text-sm text-muted">{{ t('settings.languageHint') }}</p>
          </div>
          <select
            class="h-9 min-w-[8.5rem] shrink-0 rounded-lg border border-border bg-card px-3 text-sm text-foreground outline-none focus:border-accent/50 cursor-pointer disabled:opacity-50"
            :value="uiLocale"
            :disabled="localeSaving"
            :aria-label="t('settings.language')"
            @change="onUiLocaleChange(($event.target as HTMLSelectElement).value as UiLocalePreference)"
          >
            <option v-for="opt in localeOptions" :key="opt.value" :value="opt.value">
              {{ t(opt.labelKey) }}
            </option>
          </select>
        </div>
      </div>

      <!-- 工具调用 -->
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <h4 class="text-sm font-medium text-foreground">{{ t('settings.display.toolCalls') }}</h4>
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

      <!-- {{ t('settings.display.agentOutput') }} -->
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <h4 class="text-sm font-medium text-foreground">{{ t('settings.display.agentOutput') }}</h4>
        <div class="grid grid-cols-2 gap-y-3 gap-x-32">
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">{{ t('settings.display.showReasoning') }}</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input type="checkbox" class="sr-only peer" :checked="displayUiChecked('showReasoning')" @change="setDisplayUi('showReasoning', ($event.target as HTMLInputElement).checked)" />
              <div class="settings-toggle-track" />
            </label>
          </div>
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">{{ t('settings.display.showTaskBoard') }}</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input type="checkbox" class="sr-only peer" :checked="displayUiChecked('showTaskBoardPanel')" @change="setDisplayUi('showTaskBoardPanel', ($event.target as HTMLInputElement).checked)" />
              <div class="settings-toggle-track" />
            </label>
          </div>
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">{{ t('settings.display.showSubAgentTrace') }}</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input type="checkbox" class="sr-only peer" :checked="displayUiChecked('showSubAgentTrace')" @change="setDisplayUi('showSubAgentTrace', ($event.target as HTMLInputElement).checked)" />
              <div class="settings-toggle-track" />
            </label>
          </div>
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">{{ t('settings.display.showChildBoards') }}</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input v-model="taskBoardShowChildBoards" type="checkbox" class="sr-only peer" />
              <div class="settings-toggle-track" />
            </label>
          </div>
        </div>
      </div>

      <!-- {{ t('settings.display.execution') }} -->
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <h4 class="text-sm font-medium text-foreground">{{ t('settings.display.execution') }}</h4>
        <div class="grid grid-cols-2 gap-y-3 gap-x-32">
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">{{ t('settings.display.autoCompact') }}</h4>
            <label class="relative inline-flex items-center cursor-pointer shrink-0">
              <input v-model="computerAutoCompact" type="checkbox" class="sr-only peer" />
              <div class="settings-toggle-track" />
            </label>
          </div>
          <div class="flex items-center justify-between gap-3">
            <h4 class="text-[12px] font-medium text-foreground">{{ t('settings.display.collapseProcess') }}</h4>
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
        <h4 id="system-notify-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">{{ t('settings.notify.heading') }}</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <p class="text-sm font-medium text-foreground flex items-center gap-2">
              <Volume2 class="w-4 h-4 text-accent" />{{ t('settings.notify.playSound') }}
            </p>
            <p class="mt-1 text-sm text-muted">{{ t('settings.notify.playSoundHint') }}</p>
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
        <h4 id="system-desktop-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">{{ t('settings.desktop.heading') }}</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-5">
        <div>
          <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
            <Monitor class="w-4 h-4 text-accent shrink-0" />{{ t('settings.desktop.computerBehavior') }}
          </h4>
          <p class="mt-1 text-[11px] text-muted">{{ t('settings.desktop.computerBehaviorHint') }}</p>
        </div>

        <div class="border-t border-border pt-4 space-y-0 divide-y divide-border">
          <div class="flex items-start justify-between gap-4 py-3 first:pt-0">
            <div class="min-w-0">
              <p class="text-[12px] font-medium text-foreground">{{ t('settings.desktop.humanLikeMouse') }}</p>
              <p class="text-[11px] text-muted mt-0.5">{{ t('settings.desktop.humanLikeMouseHint') }}</p>
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
              <p class="text-[12px] font-medium text-foreground">{{ t('settings.desktop.autoSwitchMonitor') }}</p>
              <p class="text-[11px] text-muted mt-0.5">{{ t('settings.desktop.autoSwitchMonitorHint') }}</p>
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
        </div>
      </div>
    </section>

    <!-- 媒体生成 -->
    <section class="space-y-4" aria-labelledby="system-media-gen-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-media-gen-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">{{ t('settings.mediaGen.heading') }}</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
          <Sparkles class="w-4 h-4 text-accent" />{{ t('settings.mediaGen.title') }}
        </h4>
        <p class="text-[11px] text-muted">{{ t('settings.mediaGen.hint') }}</p>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div>
            <label class="block text-[12px] text-muted mb-1.5 flex items-center gap-1.5">
              {{ t('settings.mediaGen.image') }}
              <span
                v-if="imageGenOverridden"
                class="rounded bg-warning/15 px-1 text-[9px] text-warning"
              >{{ t('settings.overridden') }}</span>
            </label>
            <select
              :value="mediaImageGenerationModel"
              class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground cursor-pointer outline-none focus:border-accent/50"
              @change="selectMediaModelWithProvider('imageGeneration', ($event.target as HTMLSelectElement).value)"
            >
              <option value="">{{ imageGenDefaultLabel }}</option>
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
            <label class="block text-[12px] text-muted mb-1.5 flex items-center gap-1.5">
              {{ t('settings.mediaGen.video') }}
              <span
                v-if="videoGenOverridden"
                class="rounded bg-warning/15 px-1 text-[9px] text-warning"
              >{{ t('settings.overridden') }}</span>
            </label>
            <select
              :value="mediaVideoGenerationModel"
              class="w-full h-9 px-3 rounded-lg bg-card border border-border text-sm text-foreground cursor-pointer outline-none focus:border-accent/50"
              @change="selectMediaModelWithProvider('videoGeneration', ($event.target as HTMLSelectElement).value)"
            >
              <option value="">{{ videoGenDefaultLabel }}</option>
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
        <h4 id="system-runtime-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">{{ t('settings.runtime.heading') }}</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <div class="flex items-center justify-between gap-2">
          <h4 class="text-sm font-medium text-foreground flex items-center gap-2">
            <Film class="w-4 h-4 text-accent" />{{ t('settings.runtime.mediaUnderstanding') }}
          </h4>
          <button
            type="button"
            class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
            @click="mediaDepsModalOpen = true"
          >
            {{ t('settings.runtime.manage') }}
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
            <Terminal class="w-4 h-4 text-accent" />{{ t('settings.runtime.terminalEnv') }}
          </h4>
          <button
            type="button"
            class="inline-flex items-center gap-1 h-7 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
            @click="addTerminalEnvRow(); onTerminalEnvRowChanged()"
          >
            {{ t('settings.runtime.add') }}
            <Plus class="w-3 h-3" />
          </button>
        </div>
        <p class="text-[11px] text-muted">{{ t('settings.runtime.terminalEnvHint') }}</p>

        <div v-if="terminalEnvRows.length === 0" class="rounded-lg border border-dashed border-border bg-card/40 px-3 py-4 text-center text-[11px] text-muted">
          {{ t('settings.runtime.terminalEnvEmpty') }}
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
              :placeholder="t('settings.keyAPITOKEN_105c40')"
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
              :title="t('settings.runtime.deleteEnv', { key: row.key || t('settings.runtime.envVarFallback') })"
              :aria-label="t('settings.runtime.deleteEnvAria')"
              @click="removeTerminalEnvRow(row.id); onTerminalEnvRowChanged()"
            >
              <X class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- 执行 -->
    <section class="space-y-4" aria-labelledby="system-exec-heading">
      <div class="flex items-center gap-2 px-1 pt-2 pb-1">
        <h4 id="system-exec-heading" class="text-[11px] font-semibold uppercase tracking-wider text-muted/80">{{ t('settings.exec.heading') }}</h4>
        <div class="flex-1 h-px bg-border/60" />
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5">
        <div class="grid grid-cols-1 min-[960px]:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)_auto_minmax(0,1fr)] items-stretch gap-x-5 gap-y-6">
          <div class="min-w-0">
            <div class="w-max max-w-full space-y-3">
              <div class="flex items-center justify-between gap-4 h-5">
                <span class="text-[12px] font-medium text-foreground">{{ t('settings.exec.toolParallel') }}</span>
                <label
                  class="relative inline-flex items-center cursor-pointer shrink-0"
                  :title="t('settings.exec.toolParallelHint')"
                >
                  <input
                    v-model="parallelToolExecutionEnabled"
                    type="checkbox"
                    class="sr-only peer"
                    aria-label="{{ t('settings.exec.toolParallel') }}"
                  />
                  <div class="settings-toggle-track"></div>
                </label>
              </div>
              <div v-if="parallelToolExecutionEnabled" class="flex flex-wrap items-start gap-x-5 gap-y-3">
                <div>
                  <div class="flex items-center gap-1 mb-1.5">
                    <span class="text-[12px] text-muted">{{ t('settings.exec.generalTools') }}</span>
                    <button
                      type="button"
                      class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                      :title="t('settings.exec.generalToolsHint')"
                      :aria-label="t('settings.exec.generalTools')"
                    >
                      <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                    </button>
                  </div>
                  <input
                    :value="maxParallelToolCalls"
                    type="number"
                    min="1"
                    max="64"
                    step="1"
                    class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                    @change="commitMaxParallelToolCalls(($event.target as HTMLInputElement).value)"
                    @blur="commitMaxParallelToolCalls(($event.target as HTMLInputElement).value)"
                  />
                </div>
                <div>
                  <div class="flex items-center gap-1 mb-1.5">
                    <span class="text-[12px] text-muted">{{ t('settings.exec.subAgent') }}</span>
                    <button
                      type="button"
                      class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                      :title="t('settings.exec.subAgentHint')"
                      :aria-label="t('settings.exec.subAgent')"
                    >
                      <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                    </button>
                  </div>
                  <input
                    :value="maxParallelSubAgents"
                    type="number"
                    min="1"
                    max="64"
                    step="1"
                    class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                    @change="commitMaxParallelSubAgents(($event.target as HTMLInputElement).value)"
                    @blur="commitMaxParallelSubAgents(($event.target as HTMLInputElement).value)"
                  />
                </div>
                <div>
                  <div class="flex items-center gap-1 mb-1.5">
                    <span class="text-[12px] text-muted">{{ t('settings.exec.mediaTools') }}</span>
                    <button
                      type="button"
                      class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                      :title="t('settings.exec.mediaToolsHint')"
                      :aria-label="t('settings.exec.mediaTools')"
                    >
                      <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                    </button>
                  </div>
                  <input
                    :value="maxParallelMediaJobs"
                    type="number"
                    min="1"
                    max="64"
                    step="1"
                    class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                    @change="commitMaxParallelMediaJobs(($event.target as HTMLInputElement).value)"
                    @blur="commitMaxParallelMediaJobs(($event.target as HTMLInputElement).value)"
                  />
                </div>
              </div>
            </div>
          </div>

          <div class="hidden min-[960px]:block w-px bg-border shrink-0" aria-hidden="true" />

          <div class="min-w-0 space-y-3">
            <div class="flex items-center h-5">
              <span class="text-[12px] font-medium text-foreground">{{ t('settings.exec.rounds') }}</span>
            </div>
            <div class="flex flex-wrap items-start gap-x-5 gap-y-3">
              <div>
                <div class="flex items-center gap-1 mb-1.5">
                  <span class="text-[12px] text-muted">{{ t('settings.exec.thisRound') }}</span>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                    :title="t('settings.exec.thisRoundHint')"
                    :aria-label="t('settings.exec.thisRound')"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <input
                  v-model.number="maxToolRounds"
                  type="number"
                  min="1"
                  max="10000"
                  step="1"
                  class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
              <div>
                <div class="flex items-center gap-1 mb-1.5">
                  <span class="text-[12px] text-muted">{{ t('settings.exec.subTask') }}</span>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                    :title="t('settings.exec.subTaskHint')"
                    :aria-label="t('settings.exec.subTask')"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <input
                  v-model.number="maxSubAgentToolRounds"
                  type="number"
                  min="1"
                  max="500"
                  step="1"
                  class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
            </div>
          </div>

          <div class="hidden min-[960px]:block w-px bg-border shrink-0" aria-hidden="true" />

          <div class="min-w-0 space-y-3">
            <div class="flex items-center h-5">
              <span class="text-[12px] font-medium text-foreground">{{ t('settings.exec.taskParallel') }}</span>
            </div>
            <div>
              <div class="flex items-center gap-1 mb-1.5">
                <span class="text-[12px] text-muted">{{ t('settings.exec.concurrentTasks') }}</span>
                <button
                  type="button"
                  class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                  :title="t('settings.exec.concurrentTasksHint')"
                  :aria-label="t('settings.exec.concurrentTasks')"
                >
                  <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                </button>
              </div>
              <div class="flex items-center gap-2">
                <input
                  v-model.number="maxConcurrentRuns"
                  type="number"
                  min="1"
                  max="64"
                  step="1"
                  class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                />
                <button
                  type="button"
                  class="inline-flex items-center gap-1.5 h-9 px-2.5 rounded-lg bg-accent/10 text-[11px] font-medium text-accent hover:bg-accent/20 cursor-pointer transition-colors shrink-0"
                  :title="queueStatusTitle"
                  :aria-label="t('settings.exec.viewQueueAria', { status: queueStatusTitle })"
                  @click="queueModalOpen = true"
                >
                  <span
                    class="w-1.5 h-1.5 rounded-full shrink-0"
                    :class="queueBusy
                      ? 'bg-accent animate-pulse'
                      : 'bg-muted'"
                    aria-hidden="true"
                  />
                  {{ t('settings.exec.viewQueue') }}
                  <ChevronRight class="w-3 h-3" />
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5">
        <div class="grid grid-cols-1 min-[960px]:grid-cols-[max-content_auto_max-content_minmax(2.5rem,1fr)] items-stretch gap-x-8 gap-y-6">
          <div class="w-max max-w-full space-y-3">
            <div class="flex items-center gap-1 h-5">
              <span class="text-[12px] font-medium text-foreground">{{ t('settings.exec.contentLimits') }}</span>
              <button
                type="button"
                class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                :title="t('settings.exec.contentLimitsHint')"
                :aria-label="t('settings.exec.contentLimits')"
              >
                <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
              </button>
            </div>
            <div class="flex flex-wrap min-[960px]:flex-nowrap items-start gap-x-5 gap-y-3">
              <div>
                <div class="flex items-center gap-1 mb-1.5">
                  <span class="text-[12px] text-muted">{{ t('settings.exec.uploadMb') }}</span>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                    :title="t('settings.exec.uploadHint')"
                    :aria-label="t('settings.exec.uploadMb')"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <input
                  v-model.number="attachmentUploadMaxMb"
                  type="number"
                  min="1"
                  max="512"
                  step="1"
                  class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
              <div>
                <div class="flex items-center gap-1 mb-1.5">
                  <span class="text-[12px] text-muted">{{ t('settings.exec.bodyKb') }}</span>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                    :title="t('settings.exec.bodyHint')"
                    :aria-label="t('settings.exec.bodyKb')"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <input
                  v-model.number="fileReadMaxKb"
                  type="number"
                  min="4"
                  max="1024"
                  step="1"
                  class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
              <div>
                <div class="flex items-center gap-1 mb-1.5">
                  <span class="text-[12px] text-muted">{{ t('settings.exec.lineBytes') }}</span>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                    :title="t('settings.exec.lineHint')"
                    :aria-label="t('settings.exec.lineBytes')"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <input
                  v-model.number="fileLineMaxBytes"
                  type="number"
                  min="256"
                  max="16384"
                  step="1"
                  class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
              <div>
                <div class="flex items-center gap-1 mb-1.5">
                  <span class="text-[12px] text-muted">{{ t('settings.exec.grepCount') }}</span>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                    :title="t('settings.exec.grepHint')"
                    :aria-label="t('settings.exec.grepCount')"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <input
                  v-model.number="fileGrepMaxResults"
                  type="number"
                  min="1"
                  max="200"
                  step="1"
                  class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
              <div>
                <div class="flex items-center gap-1 mb-1.5">
                  <span class="text-[12px] text-muted">{{ t('settings.exec.terminalKb') }}</span>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                    :title="t('settings.exec.terminalHint')"
                    :aria-label="t('settings.exec.terminalKb')"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <input
                  v-model.number="terminalOutputMaxKb"
                  type="number"
                  min="4"
                  max="256"
                  step="1"
                  class="w-20 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                />
              </div>
            </div>
          </div>

          <div class="hidden min-[960px]:block w-px bg-border shrink-0" aria-hidden="true" />

          <div class="w-max max-w-full space-y-3">
            <div class="flex items-center gap-1 h-5">
              <span class="text-[12px] font-medium text-foreground">{{ t('settings.exec.terminalTimeout') }}</span>
              <button
                type="button"
                class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                :title="t('settings.exec.terminalTimeoutHint')"
                :aria-label="t('settings.exec.terminalTimeout')"
              >
                <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
              </button>
            </div>
            <div class="flex flex-nowrap items-start gap-x-5">
              <div>
                <div class="flex items-center gap-1 mb-1.5">
                  <span class="text-[12px] text-muted">{{ t('settings.exec.idleSec') }}</span>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                    :title="t('settings.exec.idleHint')"
                    :aria-label="t('settings.exec.idleSec')"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <input
                  v-model.number="terminalTimeoutSeconds"
                  type="number"
                  min="1"
                  max="86400"
                  step="1"
                  class="w-24 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                  @blur="terminalTimeoutSeconds = restoreTimeoutSeconds(terminalTimeoutSeconds)"
                />
              </div>
              <div>
                <div class="flex items-center gap-1 mb-1.5">
                  <span class="text-[12px] text-muted">{{ t('settings.exec.maxWallHours') }}</span>
                  <button
                    type="button"
                    class="inline-flex items-center text-muted hover:text-foreground transition-colors"
                    :title="t('settings.exec.maxWallHint')"
                    :aria-label="t('settings.exec.maxWallHours')"
                  >
                    <CircleHelp class="w-3.5 h-3.5 pointer-events-none" />
                  </button>
                </div>
                <input
                  v-model.number="terminalMaxWallHours"
                  type="number"
                  min="1"
                  max="10000"
                  step="1"
                  class="w-24 h-9 px-2 rounded-lg bg-card border border-border text-sm tabular-nums text-foreground outline-none focus:border-accent/50 transition-colors"
                  @blur="terminalMaxWallHours = restoreWallHours(terminalMaxWallHours)"
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- {{ t('settings.toolPerm.heading') }} -->
    <section class="space-y-4" aria-labelledby="system-tool-approval-heading">
      <div class="rounded-xl border border-border bg-[hsl(var(--card-elevated))] p-5 space-y-3">
        <h4 id="system-tool-approval-heading" class="text-sm font-medium text-foreground flex items-center gap-2">
          <Wrench class="w-4 h-4 text-accent" />{{ t('settings.toolPerm.heading') }}
        </h4>
        <div class="grid grid-cols-2 gap-3">
          <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'auto' ? 'border-border bg-hover' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'">
            <input v-model="toolApprovalMode" type="radio" value="auto" class="sr-only" />
            <span class="block text-sm text-foreground">{{ t('settings.toolPerm.auto') }}</span>
            <span class="mt-1 block text-[11px] text-muted">{{ t('settings.toolPerm.autoHint') }}</span>
          </label>
          <label class="rounded-xl border p-3 cursor-pointer transition-all" :class="toolApprovalMode === 'manual' ? 'border-border bg-hover' : 'border-border bg-[hsl(var(--card-elevated))] hover:border-border'">
            <input v-model="toolApprovalMode" type="radio" value="manual" class="sr-only" />
            <span class="block text-sm text-foreground">{{ t('settings.toolPerm.manual') }}</span>
            <span class="mt-1 block text-[11px] text-muted">{{ t('settings.toolPerm.manualHint') }}</span>
          </label>
        </div>
      </div>
    </section>

    <!-- queue modal -->
    <Teleport to="body">
      <div
        v-if="queueModalOpen"
        class="pointer-events-auto fixed inset-0 z-[10001] flex items-center justify-center bg-foreground/32 p-4"
        role="presentation"
        @click.self="queueModalOpen = false"
      >
        <div class="w-full max-w-md max-h-[80vh] flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl" @click.stop>
          <div class="flex items-start justify-between gap-2 border-b border-border px-5 py-4 shrink-0">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">{{ t('settings.queue.title') }}</h4>
              <p class="mt-0.5 text-[11px] text-muted">{{ queueStatusTitle }}</p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              :aria-label="t('common.close')"
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
                  <span class="shrink-0 rounded px-1 py-0.5 bg-hover text-muted text-[10px]">{{ triggerSourceLabel(w.triggerSource) }}</span>
                  <span class="truncate">{{ shortId(w.conversationId, 28) }}</span>
                </li>
              </ul>
            </div>

            <div
              v-if="queueSnapshot && pendingRunCount > 0"
              class="rounded-lg border border-border bg-[hsl(var(--card-elevated))] px-3 py-2 space-y-1.5"
            >
              <div class="text-[12px] font-medium text-foreground">{{ t('settings.queue.pending') }}</div>
              <ul class="space-y-1 max-h-36 overflow-y-auto">
                <li
                  v-for="run in queueSnapshot.pendingRuns"
                  :key="run.runId"
                  class="text-[11px] text-muted flex items-center gap-1.5 min-w-0"
                  :title="run.runId"
                >
                  <span class="shrink-0 rounded px-1 py-0.5 bg-hover text-muted text-[10px]">{{ triggerSourceLabel(run.triggerSource) }}</span>
                  <span class="truncate flex-1">{{ shortId(run.conversationId, 24) }}</span>
                  <span class="shrink-0 text-[10px] text-muted/70">{{ new Date(run.createdAtMs).toLocaleTimeString() }}</span>
                </li>
              </ul>
            </div>

            <p
              v-if="queueSnapshot && pendingRunCount === 0 && !totalLaneWaiting"
              class="text-[11px] text-muted text-center py-4"
            >
              {{ t('settings.queue.empty') }}
            </p>
            <p v-else-if="!queueSnapshot && !queueLoading" class="text-[11px] text-muted text-center py-4">
              {{ t('settings.queue.unavailable') }}
            </p>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- ffmpeg modal -->
    <Teleport to="body">
      <div
        v-if="mediaDepsModalOpen"
        class="pointer-events-auto fixed inset-0 z-[10001] flex items-center justify-center bg-foreground/32 p-4"
        role="presentation"
        @click.self="mediaDepsModalOpen = false"
      >
        <div class="w-full max-w-md rounded-xl border border-border bg-card shadow-2xl p-5 space-y-4" @click.stop>
          <div class="flex items-start justify-between gap-2">
            <div class="min-w-0">
              <h4 class="text-sm font-semibold text-foreground">{{ t('settings.ffmpeg.modalTitle') }}</h4>
              <p class="mt-0.5 text-[11px] text-muted">{{ t('settings.ffmpeg.modalHint') }}</p>
            </div>
            <button
              type="button"
              class="p-1.5 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors shrink-0"
              :aria-label="t('common.close')"
              @click="mediaDepsModalOpen = false"
            >
              <X class="w-4 h-4" />
            </button>
          </div>
          <div class="rounded-lg border border-border bg-card/50 px-3 py-2.5 space-y-1">
            <p class="text-[12px] font-medium text-foreground">ffmpeg / ffprobe</p>
            <p class="text-[11px] text-muted">{{ t('settings.ffmpeg.needInstall') }}</p>
            <p
              class="text-[11px] mt-1"
              :class="mediaDeps?.status === 'ready' ? 'text-success' : 'text-warning'"
            >
              {{ ffmpegStatusLabel }}
            </p>
            <p v-if="ffmpegStatusDetail" class="text-[10px] text-muted mt-0.5 break-all">
              {{ ffmpegStatusDetail }}
            </p>
            <p v-if="mediaDeps?.status === 'ready'" class="text-[10px] text-muted mt-0.5">
              {{ t('settings.ffmpeg.frameFailNote') }}
            </p>
          </div>
          <div class="flex items-center justify-end gap-2">
            <button
              type="button"
              class="h-8 px-3 rounded-lg border border-border text-xs text-foreground hover:bg-muted/50 cursor-pointer transition-colors"
              @click="refreshMediaDeps()"
            >
              {{ t('settings.ffmpeg.redetect') }}
            </button>
            <button
              v-if="ffmpegNeedsInstall"
              type="button"
              class="h-8 px-3 rounded-lg bg-accent text-accent-foreground text-xs hover:opacity-90 cursor-pointer transition-colors"
              @click="askAssistantInstallFfmpeg()"
            >
              {{ t('settings.ffmpeg.askInstall') }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
