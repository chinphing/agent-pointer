<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { parseMarkdown } from '../../lib/markdownConfig'
import {
  Wrench,
  ChevronDown,
  ChevronRight,
  XCircle,
  Loader2,
  ShieldAlert,
  Check,
  X,
  FileEdit
} from 'lucide-vue-next'
import type { ToolCall, WebSearchSourceEntry } from '../../types/chat'
import { useChatStore } from '../../stores/chat'
import { taskBoardToolSummary, taskBoardPatchSummaryFromArgs } from '../../lib/messageTooling'
import { fileToolDisplayPath, truncateToolSummary, effectiveToolDisplayLabel, effectiveToolDisplaySummary } from '../../lib/toolCallDisplay'
import { openExternalUrl } from '../../lib/openExternalUrl'
import { useMarkdownExternalLinks } from '../../composables/useMarkdownExternalLinks'
import DiffView from './DiffView.vue'
import AskUserOptions from './AskUserOptions.vue'



const props = defineProps<{
  toolCall: ToolCall
  showToolCallResults?: boolean
  isSearchMatch?: boolean
  isActiveSearchMatch?: boolean
}>()
const chat = useChatStore()
const open = ref(false)

watch(
  () => props.isActiveSearchMatch,
  active => {
    if (active) open.value = true
  },
  { immediate: true }
)

const isTerminal = computed(() => props.toolCall.name === 'terminal')
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
const boardSummary = computed(() => taskBoardToolSummary(props.toolCall.result))

const displayLabel = computed(() => effectiveToolDisplayLabel(props.toolCall))
const displaySummary = computed(() => {
  const filePath = fileToolDisplayPath(props.toolCall, chat.current?.workspaceRoot)
  if (filePath) return truncateToolSummary(filePath)
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
    JSON.parse(text)
    return ''
  } catch (error) {
    return error instanceof Error ? error.message : '无效 JSON'
  }
})

const prettyArgs = computed(() => {
  if (isTerminal.value) return ''
  const text = props.toolCall.arguments?.trim()
  if (!text) return ''
  try {
    return JSON.stringify(JSON.parse(text), null, 2)
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

type FileEditResult = {
  path?: string
  success?: boolean
  diff_lines?: {
    type: string
    text: string
    hidden?: string[]
  }[]
  diff_stats?: {
    adds: number
    dels: number
  }
}

const fileEditResult = computed<FileEditResult | null>(() => {
  if (!isFileEdit.value && !isFileWrite.value) return null
  if (!props.toolCall.result) return null
  try {
    return JSON.parse(props.toolCall.result) as FileEditResult
  } catch {
    return null
  }
})

const fileEditFileName = computed(() => {
  const r = fileEditResult.value
  if (r?.path) {
    const parts = r.path.replace(/\\/g, '/').split('/')
    return parts[parts.length - 1] || r.path
  }
  return ''
})

const terminalMeta = computed(() => {
  const result = terminalResult.value
  if (!result) return ''
  const items = []
  if (typeof result.exitCode !== 'undefined' && result.exitCode !== null) items.push(`exit ${result.exitCode}`)
  if (result.timedOut) items.push('timeout')
  if (result.elevationDenied) items.push('已拒绝提权')
  if (result.runAborted) items.push('已结束命令')
  if (result.cancelled) items.push('已停止')
  return items.join(' · ')
})

const effectiveStatus = computed(() => {
  if (!isTerminal.value || props.toolCall.status !== 'success') return props.toolCall.status
  const r = terminalResult.value
  if (r?.timedOut === true) return 'failed' as ToolCall['status']
  if (r?.elevationDenied === true) return 'failed' as ToolCall['status']
  if (r?.runAborted === true || r?.cancelled === true) return 'failed' as ToolCall['status']
  const code = r?.exitCode
  if (typeof code === 'number' && code !== 0) return 'failed' as ToolCall['status']
  return props.toolCall.status
})

const statusInfo = computed(() => {
  if (props.toolCall.waitingForInput) {
    return { label: '等待你的输入', color: 'text-warning' }
  }
  switch (effectiveStatus.value) {
    case 'pending_approval': return { label: '等待确认', color: 'text-warning' }
    case 'running': return { label: '执行中', color: 'text-accent' }
    case 'success': return { label: '成功', color: 'text-success' }
    case 'failed': return { label: '失败', color: 'text-muted/45' }
    case 'rejected': return { label: '已拒绝', color: 'text-slate-400' }
  }
  return { label: '', color: '' }
})

function approve(ok: boolean) {
  chat.approve(props.toolCall, ok)
}

function abortTerminalOnly() {
  chat.abortTerminalOnly(props.toolCall.id)
}

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
    class="tool-call-row transition-colors"
    :data-tool-call-id="toolCall.id"
    :class="isActiveSearchMatch
      ? 'rounded-lg ring-2 ring-accent/60 bg-accent/10'
      : isSearchMatch
        ? 'rounded-lg bg-accent/5'
        : ''"
  >
    <div class="flex items-center gap-1.5 min-w-0">
      <button
        type="button"
        class="tool-call-trigger py-1 flex flex-wrap items-center gap-x-1.5 gap-y-0.5 min-w-0 text-[11px] text-muted hover:text-foreground/75 transition-colors cursor-pointer text-left"
        @click="open = !open"
      >
        <Wrench class="w-3 h-3 text-muted/70 shrink-0" />
        <span>{{ displayLabel }}</span>
        <span v-if="displaySummary">· {{ displaySummary }}</span>
        <span
          v-if="terminalElevated"
          class="shrink-0 text-[10px] text-warning inline-flex items-center gap-0.5"
        >
          <ShieldAlert class="w-2.5 h-2.5" />提权
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
        <span v-if="toolCall.durationMs" class="shrink-0 text-[10px] text-muted/45 tabular-nums">{{ toolCall.durationMs }}ms</span>
        <component
          :is="open ? ChevronDown : ChevronRight"
          class="tool-call-chevron w-3 h-3 shrink-0 ml-[2ch] text-muted hidden"
        />
      </button>
      <button
        v-if="canViewTerminalLive"
        type="button"
        class="shrink-0 border-0 bg-transparent px-0.5 py-1 text-[11px] text-accent hover:text-accent/80 cursor-pointer transition-colors"
        title="查看终端输出"
        @click="viewTerminalLive"
      >查看</button>
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
        提权命令：允许后还会在系统中弹出管理员确认（UAC / 密码 / polkit）。
      </p>
      <p
        v-if="isVideoGenerate && videoGenerateDuration"
        class="text-[11px] text-warning leading-relaxed"
      >
        本次视频生成预估花费 {{ videoGenerateDuration }} 元，您确认要生成吗？
      </p>
      <p
        v-else-if="isVideoGenerate"
        class="text-[11px] text-warning leading-relaxed"
      >
        本次视频生成按 1 元/秒计费，您确认要生成吗？
      </p>
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="h-8 px-3 rounded-lg bg-success/20 hover:bg-success/30 text-success text-xs flex items-center gap-1.5 cursor-pointer transition"
          @click="approve(true)"
        ><Check class="w-3.5 h-3.5" />允许</button>
        <button
          type="button"
          class="h-8 px-3 rounded-lg bg-danger/15 hover:bg-danger/25 text-danger text-xs flex items-center gap-1.5 cursor-pointer transition"
          @click="approve(false)"
        ><X class="w-3.5 h-3.5" />拒绝</button>
      </div>
    </div>

    <div v-if="open" class="pb-2 space-y-2">
      <template v-if="isTerminal">
        <div>
          <div class="flex items-center justify-between text-[10px] uppercase tracking-wider text-slate-500 mb-1">
            <span>执行命令</span>
            <button
              v-if="effectiveStatus === 'running'"
              type="button"
              class="normal-case tracking-normal h-6 px-2 rounded-md bg-danger/15 hover:bg-danger/25 text-danger text-[11px] cursor-pointer transition"
              @click.stop="abortTerminalOnly"
            >结束命令</button>
          </div>
          <pre class="text-[12px] bg-black/60 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-green-400 font-mono max-h-64">{{ terminalCommand || '—' }}</pre>
        </div>
        <div v-if="showResults && (toolCall.result || toolCall.terminalOutput)">
          <div class="flex items-center justify-between text-[10px] uppercase tracking-wider text-slate-500 mb-1">
            <span>控制台输出</span>
            <span v-if="terminalMeta" class="normal-case tracking-normal">{{ terminalMeta }}</span>
          </div>
          <pre class="text-[12px] bg-black/60 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-slate-200 max-h-64">{{ terminalOutput || '—' }}</pre>
        </div>
      </template>

      <template v-else-if="isWebSearch">
        <div>
          <div class="text-[10px] uppercase tracking-wider text-slate-500 mb-1">搜索问题</div>
          <pre class="text-[12px] bg-black/40 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-slate-200">{{ webSearchQuery || '—' }}</pre>
        </div>
        <div v-if="webSearchSourcesView.length">
          <div class="text-[10px] uppercase tracking-wider text-slate-500 mb-1">来源</div>
          <ul class="text-[12px] space-y-1.5 text-slate-300">
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
          <div class="text-[10px] uppercase tracking-wider text-slate-500 mb-1">回答</div>
          <div
            v-if="webSearchAnswerHtml"
            ref="webSearchAnswerRef"
            class="md-body text-[12px] bg-black/40 rounded-lg p-2.5 border border-white/5 text-slate-200 max-h-64 overflow-y-auto"
            v-html="webSearchAnswerHtml"
          />
          <pre
            v-else
            class="text-[12px] bg-black/40 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-slate-200 max-h-64 whitespace-pre-wrap"
          >{{ webSearchOutput || (effectiveStatus === 'running' ? '…' : '—') }}</pre>
        </div>
      </template>

      <template v-else-if="(isFileEdit || isFileWrite) && fileEditResult?.success && fileEditResult?.diff_lines">
        <DiffView
          :diff-lines="fileEditResult.diff_lines as any"
          :diff-stats="fileEditResult.diff_stats"
        />
      </template>

      <template v-else>
        <div>
          <div
            class="text-[10px] uppercase tracking-wider mb-1"
            :class="argsParseError ? 'text-danger' : 'text-slate-500'"
          >{{ argsParseError ? '参数解析失败 · 原始参数' : '参数' }}</div>
          <div v-if="argsParseError" class="text-[11px] text-danger mb-1 break-words">{{ argsParseError }}</div>
          <pre class="text-[12px] bg-black/40 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-slate-200">{{ prettyArgs || '—' }}</pre>
        </div>
        <div v-if="showResults && toolCall.result">
          <div class="text-[10px] uppercase tracking-wider text-slate-500 mb-1">结果</div>
          <pre class="text-[12px] bg-black/40 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-slate-200 max-h-48">{{ toolCall.result }}</pre>
        </div>
      </template>

      <div v-if="toolCall.error" class="text-[12px] text-danger">{{ toolCall.error }}</div>

      <p v-if="open" class="text-[10px] text-muted font-mono truncate">{{ toolCall.name }}</p>
    </div>
  </div>
</template>
