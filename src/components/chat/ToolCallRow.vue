<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { parseMarkdown } from '../../lib/markdownConfig'
import {
  ChevronDown,
  ChevronRight,
  XCircle,
  Loader2,
  ShieldAlert,
  Check,
  X
} from 'lucide-vue-next'
import type { ToolCall, WebSearchSourceEntry } from '../../types/chat'
import { useChatStore } from '../../stores/chat'
import { taskBoardToolSummary, taskBoardPatchSummaryFromArgs, toolCallBaseName } from '../../lib/messageTooling'
import { fileToolDisplayPath, truncateToolSummary, compactToolCallStatusLine, effectiveToolDisplayLabel, effectiveToolDisplaySummary, formatToolDurationLabel, isBackgroundJobHandleResult, isBackgroundJobHost, isBackgroundSubagentCall, backgroundJobIdFromToolCall, isJobAwaitCall, resolveBackgroundHostDisplayStatus } from '../../lib/toolCallDisplay'
import { toolCallShowsKindLabel } from '../../lib/toolCallKindIcon'
import ToolKindIcon from './ToolKindIcon.vue'
import ToolLiveSweepText from './ToolLiveSweepText.vue'
import { openExternalUrl } from '../../lib/openExternalUrl'
import { useMarkdownExternalLinks } from '../../composables/useMarkdownExternalLinks'
import DiffView from './DiffView.vue'
import AskUserOptions from './AskUserOptions.vue'
import { parseToolCallArguments } from '../../lib/parseToolCallArguments'
import { computeDiffLines, fileEditSnippetFromArgs } from '../../lib/textDiff'



const props = defineProps<{
  toolCall: ToolCall
  showToolCallResults?: boolean
  isSearchMatch?: boolean
  isActiveSearchMatch?: boolean
  /** Match collapsed group line height while this row is the live trailing tool. */
  dense?: boolean
}>()
const { t } = useI18n()
const chat = useChatStore()
const open = ref(false)

watch(open, isOpen => {
  if (isOpen) void chat.ensureToolCallBody(props.toolCall.id)
  else chat.releaseToolCallBody(props.toolCall.id)
})

watch(
  () => props.toolCall.bodyEvicted,
  evicted => {
    if (evicted && open.value) open.value = false
  }
)

watch(
  () => props.isActiveSearchMatch,
  active => {
    if (active) open.value = true
  },
  { immediate: true }
)

watch(
  () => props.toolCall.id,
  (_next, prev) => {
    if (prev && open.value) chat.releaseToolCallBody(prev)
    open.value = false
  }
)

onUnmounted(() => {
  if (open.value) chat.releaseToolCallBody(props.toolCall.id)
})

const isTerminal = computed(() => props.toolCall.name === 'terminal')
const isRunSubagent = computed(() => toolCallBaseName(props.toolCall.name) === 'run_subagent')
const isLivePulse = computed(
  () =>
    effectiveStatus.value === 'running'
    && !props.toolCall.waitingForInput
    && !isRunSubagent.value
)
const isWebSearch = computed(() => props.toolCall.name === 'web_search')
const isFileEdit = computed(() => props.toolCall.name === 'file_edit')
const isFileWrite = computed(() => props.toolCall.name === 'file_write')
const isVideoGenerate = computed(() => props.toolCall.name === 'video_generate')

const videoGenerateDuration = computed(() => {
  if (!isVideoGenerate.value) return null
  try {
    const args = JSON.parse(props.toolCall.arguments || '{}')
    const dur = args.durationSeconds ?? args.duration_seconds
    if (typeof dur === 'number' && dur > 0) return dur
  } catch { /* ignore */ }
  return null
})
const boardSummary = computed(() => {
  // Only parse `result` for task_board-family tools; other tools (e.g. ask_user's
  // {"selected": ...}) must never render as "Task board · …".
  const name = props.toolCall.name
  const base = name.indexOf(':') === -1 ? name : name.slice(0, name.indexOf(':'))
  if (!base.startsWith('task_board')) return null
  return taskBoardToolSummary(props.toolCall.result)
})

const displayLabel = computed(() => effectiveToolDisplayLabel(props.toolCall))
const showKindLabel = computed(() => toolCallShowsKindLabel(props.toolCall.name))
const rowAriaLabel = computed(() =>
  compactToolCallStatusLine(props.toolCall, chat.current?.workspaceRoot, {
    includeStatus: false
  })
)
const durationLabel = computed(() => formatToolDurationLabel(props.toolCall.durationMs))
const filePathSummary = computed(() =>
  fileToolDisplayPath(props.toolCall, chat.current?.workspaceRoot)
)
const displaySummary = computed(() => {
  if (filePathSummary.value) return filePathSummary.value
  const s = effectiveToolDisplaySummary(props.toolCall)
  if (s) return s
  if (showResults.value) {
    const board = boardSummary.value ?? ''
    return board ? truncateToolSummary(board) : ''
  }
  return ''
})

const showStatusLabel = computed(
  () =>
    effectiveStatus.value === 'running'
    || effectiveStatus.value === 'pending_approval'
    || effectiveStatus.value === 'rejected'
    || props.toolCall.waitingForInput === true
)

const showSuccessQuiet = computed(() => effectiveStatus.value === 'success')
const showFailedQuiet = computed(() => effectiveStatus.value === 'failed')
const showResults = computed(() => props.showToolCallResults === true)

const terminalArgs = computed(() => {
  if (!isTerminal.value) return null
  const text = props.toolCall.arguments?.trim()
  if (!text) return null
  try {
    return JSON.parse(text) as { command?: string; elevated?: boolean }
  } catch {
    return null
  }
})

const terminalElevated = computed(
  () => terminalArgs.value?.elevated === true
)

const terminalCommand = computed(() => {
  if (!isTerminal.value) return ''
  const fromArgs = terminalArgs.value?.command
  if (fromArgs) return fromArgs
  const text = props.toolCall.arguments?.trim()
  return text || ''
})

const argsParseError = computed(() => {
  if (isTerminal.value) return ''
  const text = props.toolCall.arguments?.trim()
  if (!text) return ''
  try {
    parseToolCallArguments(text)
    return ''
  } catch (error) {
    return error instanceof Error ? error.message : t('tools.args.invalidJson')
  }
})

const prettyArgs = computed(() => {
  if (isTerminal.value) return ''
  const text = props.toolCall.arguments?.trim()
  if (!text) return ''
  try {
    return JSON.stringify(parseToolCallArguments(text), null, 2)
  } catch { return text }
})

type TerminalResult = {
  stdout?: string
  stderr?: string
  exitCode?: number | null
  timedOut?: boolean
  cancelled?: boolean
  runAborted?: boolean
  elevationDenied?: boolean
}

const terminalResult = computed<TerminalResult | null>(() => {
  if (!isTerminal.value || !props.toolCall.result) return null
  if (isBackgroundJobHandleResult(props.toolCall.result)) return null
  try {
    return JSON.parse(props.toolCall.result) as TerminalResult
  } catch {
    return { stdout: props.toolCall.result }
  }
})

const terminalOutput = computed(() => {
  if (props.toolCall.terminalOutput) return props.toolCall.terminalOutput
  const result = terminalResult.value
  if (!result) return ''
  const parts = []
  if (result.stdout) parts.push(result.stdout)
  if (result.stderr) parts.push(result.stderr)
  return parts.join(result.stdout && result.stderr ? '\n' : '')
})

/** Must be declared before any computed/watch that reads it during setup (e.g. useMarkdownExternalLinks). */
const effectiveStatus = computed(() => {
  const hostStatus = resolveBackgroundHostDisplayStatus(props.toolCall)
  if (!isTerminal.value || hostStatus !== 'success') return hostStatus
  const r = terminalResult.value
  if (r?.timedOut === true) return 'failed' as ToolCall['status']
  if (r?.elevationDenied === true) return 'failed' as ToolCall['status']
  if (r?.runAborted === true || r?.cancelled === true) return 'failed' as ToolCall['status']
  const code = r?.exitCode
  if (typeof code === 'number' && code !== 0) return 'failed' as ToolCall['status']
  return hostStatus
})

const webSearchQuery = computed(() => {
  if (!isWebSearch.value) return ''
  const text = props.toolCall.arguments?.trim()
  if (!text) return ''
  try {
    const parsed = JSON.parse(text)
    return parsed.query || ''
  } catch { return text }
})

const webSearchOutput = computed(() => {
  if (!isWebSearch.value) return ''
  if (props.toolCall.result && effectiveStatus.value === 'success') {
    try {
      const parsed = JSON.parse(props.toolCall.result)
      if (typeof parsed.answer === 'string' && parsed.answer.trim()) return parsed.answer
    } catch { /* fall through */ }
  }
  if (props.toolCall.webSearchOutput) return props.toolCall.webSearchOutput
  if (!props.toolCall.result) return ''
  try {
    const parsed = JSON.parse(props.toolCall.result)
    return typeof parsed.answer === 'string' ? parsed.answer : ''
  } catch {
    return ''
  }
})

const webSearchAnswerRef = ref<HTMLElement | null>(null)
const webSearchAnswerHtml = computed(() => {
  const text = webSearchOutput.value
  if (!text.trim() || effectiveStatus.value !== 'success') return ''
  return parseMarkdown(text)
})

useMarkdownExternalLinks(webSearchAnswerRef, () => webSearchOutput.value)

function sourceSiteLabel(source: WebSearchSourceEntry): string {
  const named = source.siteName?.trim()
  if (named) return named
  try {
    return new URL(source.url).hostname.replace(/^www\./i, '')
  } catch {
    return ''
  }
}

const webSearchSources = computed((): WebSearchSourceEntry[] => {
  if (props.toolCall.webSearchSources?.length) return props.toolCall.webSearchSources
  if (!isWebSearch.value || !props.toolCall.result) return []
  try {
    const parsed = JSON.parse(props.toolCall.result) as { sources?: unknown }
    if (!Array.isArray(parsed.sources)) return []
    return parsed.sources.filter(
      (item): item is WebSearchSourceEntry =>
        !!item
        && typeof item === 'object'
        && typeof (item as WebSearchSourceEntry).index === 'number'
        && typeof (item as WebSearchSourceEntry).url === 'string'
    )
  } catch {
    return []
  }
})

const webSearchSourcesView = computed(() =>
  webSearchSources.value.map((source: WebSearchSourceEntry) => ({
    source,
    siteLabel: sourceSiteLabel(source)
  }))
)

type FileMutateResult = {
  path?: string
  success?: boolean
  stats?: { adds?: number; dels?: number }
  diff_stats?: { adds?: number; dels?: number }
}

const fileMutateResult = computed<FileMutateResult | null>(() => {
  if (!isFileEdit.value && !isFileWrite.value) return null
  if (!props.toolCall.result) return null
  try {
    return JSON.parse(props.toolCall.result) as FileMutateResult
  } catch {
    return null
  }
})

const fileEditSnippetDiff = computed(() => {
  if (!isFileEdit.value || fileMutateResult.value?.success !== true) return null
  const snippet = fileEditSnippetFromArgs(props.toolCall.arguments)
  if (!snippet) return null
  return computeDiffLines(snippet.oldString, snippet.newString)
})

const terminalMeta = computed(() => {
  const result = terminalResult.value
  if (!result) return ''
  const items = []
  if (typeof result.exitCode !== 'undefined' && result.exitCode !== null) items.push(`exit ${result.exitCode}`)
  if (result.timedOut) items.push('timeout')
  if (result.elevationDenied) items.push(t('tools.terminal.elevationDenied'))
  if (result.runAborted) items.push(t('tools.terminal.aborted'))
  if (result.cancelled) items.push(t('tools.terminal.cancelled'))
  return items.join(' · ')
})

const statusInfo = computed(() => {
  if (props.toolCall.waitingForInput) {
    return { label: t('tools.status.waitingForInput'), color: 'text-warning' }
  }
  switch (effectiveStatus.value) {
    case 'pending_approval': return { label: t('tools.status.pendingApproval'), color: 'text-warning' }
    case 'running': return {
      label: isBackgroundJobHost(props.toolCall) ? t('tools.progress.runningBackground') : t('tools.progress.running'),
      color: 'text-accent'
    }
    case 'success': {
      if (isJobAwaitCall(props.toolCall)) {
        try {
          const parsed = JSON.parse(props.toolCall.result || '') as { reason?: string }
          if (parsed.reason === 'wait_ended') {
            return { label: t('tools.status.waitEnded'), color: 'text-muted' }
          }
        } catch {
          /* ignore */
        }
      }
      return {
        label: isBackgroundSubagentCall(props.toolCall) ? t('tools.status.completed') : t('tools.status.success'),
        color: 'text-success'
      }
    }
    case 'failed': {
      // Legacy stored error text may still contain the Chinese cancellation marker.
      const cancelled = /cancel|interrupted|已停止/i.test(props.toolCall.error || '')
      if (isBackgroundSubagentCall(props.toolCall) && cancelled) {
        return { label: t('tools.status.cancelled'), color: 'text-muted/45' }
      }
      return { label: t('tools.status.failed'), color: 'text-muted/45' }
    }
    case 'rejected': return { label: t('tools.status.rejected'), color: 'text-muted' }
  }
  return { label: '', color: '' }
})

function approve(ok: boolean) {
  chat.approve(props.toolCall, ok)
}

function abortTerminalOnly() {
  chat.abortTerminalOnly(props.toolCall.id)
}

function cancelThisBackgroundJob() {
  const jobId = backgroundJobIdFromToolCall(props.toolCall)
  if (jobId) void chat.cancelBackgroundJob(jobId)
}

function endWaitOnly() {
  void chat.endWaitKeepBackground()
}

const showEndBackgroundJob = computed(() => {
  if (effectiveStatus.value !== 'running') return false
  if (!isBackgroundJobHost(props.toolCall)) return false
  return !!backgroundJobIdFromToolCall(props.toolCall)
})

const showEndWait = computed(
  () =>
    effectiveStatus.value === 'running'
    && isJobAwaitCall(props.toolCall)
)

const canViewTerminalLive = computed(
  () =>
    isTerminal.value
    && effectiveStatus.value === 'running'
    && props.toolCall.waitingForInput !== true
    && chat.terminalLiveViewReadyToolCallId === props.toolCall.id
    && (props.toolCall.terminalOutput?.trim().length ?? 0) > 0
)

function viewTerminalLive() {
  chat.openTerminalLivePopup(props.toolCall.id)
}

function openSourceUrl(url: string) {
  void openExternalUrl(url)
}

</script>

<template>
  <div
    class="tool-call-row group min-w-0 w-full max-w-full transition-colors"
    :data-tool-call-id="toolCall.id"
    :class="isActiveSearchMatch
      ? 'rounded-lg ring-2 ring-accent/60 bg-accent/10'
      : isSearchMatch
        ? 'rounded-lg bg-accent/5'
        : ''"
  >
    <div class="flex max-w-full items-center gap-1.5 min-w-0">
      <button
        type="button"
        class="tool-call-trigger flex min-w-0 items-center gap-x-1.5 overflow-hidden text-muted hover:text-foreground/75 transition-colors cursor-pointer text-left"
        :class="dense ? 'py-0.5 text-[13px] leading-5' : 'py-1 text-[11px]'"
        :aria-expanded="open"
        :aria-label="rowAriaLabel"
        @click="open = !open"
      >
        <ToolKindIcon :name="toolCall.name" />
        <span
          v-if="showKindLabel"
          class="shrink-0"
        >{{ displayLabel }}</span>
        <template v-if="filePathSummary">
          <span v-if="showKindLabel" class="shrink-0">·</span>
          <ToolLiveSweepText
            class="ellipsis-start min-w-0"
            :text="filePathSummary"
            :active="isLivePulse"
            :title="filePathSummary"
          />
        </template>
        <ToolLiveSweepText
          v-else-if="displaySummary"
          class="min-w-0"
          :class="dense ? 'truncate' : 'break-words'"
          :text="showKindLabel ? `· ${displaySummary}` : displaySummary"
          :active="isLivePulse"
        />
        <ToolLiveSweepText
          v-else-if="!showKindLabel"
          class="shrink-0"
          :text="displayLabel"
          :active="isLivePulse"
        />
        <span
          v-if="terminalElevated"
          class="shrink-0 text-[10px] text-warning inline-flex items-center gap-0.5"
        >
          <ShieldAlert class="w-2.5 h-2.5" />{{ t('tools.terminal.elevated') }}
        </span>
        <span
          v-if="showStatusLabel"
          class="shrink-0 inline-flex items-center gap-0.5"
          :class="statusInfo.color"
        >
          <Loader2 v-if="effectiveStatus === 'running'" class="w-2.5 h-2.5 animate-spin" />
          <XCircle v-else-if="effectiveStatus === 'rejected'" class="w-2.5 h-2.5" />
          <span>{{ statusInfo.label }}</span>
        </span>
        <span v-else-if="showFailedQuiet" class="shrink-0 text-[10px] text-muted/45">{{ statusInfo.label }}</span>
        <span v-else-if="showSuccessQuiet" class="shrink-0 text-muted/45">{{ statusInfo.label }}</span>
        <span v-if="durationLabel" class="shrink-0 text-[10px] text-muted/45 tabular-nums">{{ durationLabel }}</span>
        <component
          :is="open ? ChevronDown : ChevronRight"
          class="tool-call-chevron w-3 h-3 shrink-0 text-muted"
          :class="open ? 'inline-flex' : 'hidden'"
          aria-hidden="true"
        />
      </button>
      <button
        v-if="canViewTerminalLive"
        type="button"
        class="shrink-0 border-0 bg-transparent px-0.5 py-1 text-[11px] text-accent hover:text-accent/80 cursor-pointer transition-colors"
        :title="t('tools.terminal.viewOutputTitle')"
        @click="viewTerminalLive"
      >{{ t('tools.terminal.view') }}</button>
      <button
        v-if="showEndBackgroundJob"
        type="button"
        class="shrink-0 border-0 bg-transparent px-0.5 py-1 text-[11px] text-danger/80 hover:text-danger cursor-pointer transition-all opacity-0 group-hover:opacity-100"
        :title="t('backgroundJobs.endJobOnlyTitle')"
        @click.stop="cancelThisBackgroundJob"
      >{{ t('backgroundJobs.endJob') }}</button>
      <button
        v-if="showEndWait"
        type="button"
        class="shrink-0 border-0 bg-transparent px-0.5 py-1 text-[11px] text-danger/80 hover:text-danger cursor-pointer transition-all opacity-0 group-hover:opacity-100"
        :title="t('backgroundJobs.endWaitTitle')"
        @click.stop="endWaitOnly"
      >{{ t('backgroundJobs.endWait') }}</button>
    </div>

    <AskUserOptions
      v-if="toolCall.name === 'ask_user'"
      :tool-call="toolCall"
    />

    <div
      v-if="toolCall.status === 'pending_approval'"
      class="pl-4 pb-1.5 space-y-1.5"
    >
      <p
        v-if="terminalElevated"
        class="text-[11px] text-warning leading-relaxed"
      >
        {{ t('tools.terminal.elevationWarning') }}
      </p>
      <p
        v-if="isVideoGenerate && videoGenerateDuration"
        class="text-[11px] text-warning leading-relaxed"
      >
        {{ t('tools.video.confirmCostEstimate', { duration: videoGenerateDuration }) }}
      </p>
      <p
        v-else-if="isVideoGenerate"
        class="text-[11px] text-warning leading-relaxed"
      >
        {{ t('tools.video.confirmCostPerSecond') }}
      </p>
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="h-8 px-3 rounded-lg bg-success/20 hover:bg-success/30 text-success text-xs flex items-center gap-1.5 cursor-pointer transition"
          @click="approve(true)"
        ><Check class="w-3.5 h-3.5" />{{ t('tools.allow') }}</button>
        <button
          type="button"
          class="h-8 px-3 rounded-lg bg-danger/15 hover:bg-danger/25 text-danger text-xs flex items-center gap-1.5 cursor-pointer transition"
          @click="approve(false)"
        ><X class="w-3.5 h-3.5" />{{ t('tools.reject') }}</button>
      </div>
    </div>

    <div v-if="open" class="pb-2 space-y-2">
      <template v-if="isTerminal">
        <div>
          <div class="flex items-center justify-between text-[10px] uppercase tracking-wider text-muted mb-1">
            <span>{{ t('tools.terminal.command') }}</span>
            <button
              v-if="effectiveStatus === 'running'"
              type="button"
              class="normal-case tracking-normal h-6 px-2 rounded-md bg-danger/15 hover:bg-danger/25 text-danger text-[11px] cursor-pointer transition"
              @click.stop="abortTerminalOnly"
            >{{ t('tools.terminal.abortAction') }}</button>
          </div>
          <pre class="text-[12px] bg-[hsl(var(--code-bg))] rounded-lg p-2.5 border border-border overflow-x-auto text-foreground font-mono max-h-64">{{ terminalCommand || '—' }}</pre>
        </div>
        <div v-if="showResults && (toolCall.terminalOutput || (toolCall.result && !isBackgroundJobHandleResult(toolCall.result)))">
          <div class="flex items-center justify-between text-[10px] uppercase tracking-wider text-muted mb-1">
            <span>{{ t('tools.terminal.consoleOutput') }}</span>
            <span v-if="terminalMeta" class="normal-case tracking-normal">{{ terminalMeta }}</span>
          </div>
          <pre class="text-[12px] bg-[hsl(var(--code-bg))] rounded-lg p-2.5 border border-border overflow-x-auto text-foreground max-h-64">{{ terminalOutput || '—' }}</pre>
        </div>
      </template>

      <template v-else-if="isWebSearch">
        <div>
          <div class="text-[10px] uppercase tracking-wider text-muted mb-1">{{ t('tools.webSearch.query') }}</div>
          <pre class="text-[12px] bg-[hsl(var(--code-bg))] rounded-lg p-2.5 border border-border overflow-x-auto text-foreground">{{ webSearchQuery || '—' }}</pre>
        </div>
        <div v-if="webSearchSourcesView.length">
          <div class="text-[10px] uppercase tracking-wider text-muted mb-1">{{ t('tools.webSearch.sources') }}</div>
          <ul class="text-[12px] space-y-1.5 text-foreground">
            <li v-for="{ source: s, siteLabel } in webSearchSourcesView" :key="s.url + s.index" class="min-w-0">
              <div class="flex items-baseline gap-1 min-w-0 truncate">
                <span class="text-muted shrink-0">{{ s.index }}.</span>
                <span
                  v-if="siteLabel"
                  class="text-foreground/80 shrink-0 max-w-[40%] truncate"
                  :title="siteLabel"
                >{{ siteLabel }}</span>
                <span v-if="siteLabel" class="text-muted shrink-0">·</span>
                <button
                  type="button"
                  class="text-accent hover:underline cursor-pointer truncate min-w-0 align-baseline"
                  :title="s.title || s.url"
                  @click.stop="openSourceUrl(s.url)"
                >{{ s.title || s.url }}</button>
              </div>
            </li>
          </ul>
        </div>
        <div v-if="webSearchOutput || effectiveStatus === 'running'">
          <div class="text-[10px] uppercase tracking-wider text-muted mb-1">{{ t('tools.webSearch.answer') }}</div>
          <div
            v-if="webSearchAnswerHtml"
            ref="webSearchAnswerRef"
            class="md-body text-[12px] bg-[hsl(var(--code-bg))] rounded-lg p-2.5 border border-border text-foreground max-h-64 overflow-y-auto"
            v-html="webSearchAnswerHtml"
          />
          <pre
            v-else
            class="text-[12px] bg-[hsl(var(--code-bg))] rounded-lg p-2.5 border border-border overflow-x-auto text-foreground max-h-64 whitespace-pre-wrap"
          >{{ webSearchOutput || (effectiveStatus === 'running' ? '…' : '—') }}</pre>
        </div>
      </template>

      <template v-else-if="isFileEdit && fileEditSnippetDiff">
        <DiffView
          :diff-lines="fileEditSnippetDiff.diffLines"
          :diff-stats="fileEditSnippetDiff.diffStats"
        />
      </template>

      <template v-else>
        <div>
          <div
            class="text-[10px] uppercase tracking-wider mb-1"
            :class="argsParseError ? 'text-danger' : 'text-muted'"
          >{{ argsParseError ? t('tools.args.parseFailed') : t('tools.args.label') }}</div>
          <div v-if="argsParseError" class="text-[11px] text-danger mb-1 break-words">{{ argsParseError }}</div>
          <pre class="text-[12px] bg-[hsl(var(--code-bg))] rounded-lg p-2.5 border border-border overflow-x-auto text-foreground">{{ prettyArgs || '—' }}</pre>
        </div>
        <div v-if="showResults && toolCall.result && !isBackgroundSubagentCall(toolCall) && !isBackgroundJobHandleResult(toolCall.result)">
          <div class="text-[10px] uppercase tracking-wider text-muted mb-1">{{ t('tools.result.label') }}</div>
          <pre class="text-[12px] bg-[hsl(var(--code-bg))] rounded-lg p-2.5 border border-border overflow-x-auto text-foreground max-h-48">{{ toolCall.result }}</pre>
        </div>
      </template>

      <div v-if="toolCall.error" class="text-[12px] text-danger">{{ toolCall.error }}</div>

      <p v-if="open" class="text-[10px] text-muted font-mono truncate">{{ toolCall.name }}</p>
    </div>
  </div>
</template>
