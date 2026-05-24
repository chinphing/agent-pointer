<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Wrench, ChevronDown, ChevronRight, CheckCircle2, XCircle, Loader2, ShieldAlert, Check, X } from 'lucide-vue-next'
import type { ToolCall } from '../../types/chat'
import { useChatStore } from '../../stores/chat'
import { taskBoardToolSummary } from '../../lib/messageTooling'

const props = defineProps<{
  toolCall: ToolCall
  showToolCallResults?: boolean
}>()
const chat = useChatStore()
const open = ref(false)
let autoCollapseTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.toolCall,
  () => {
    if (autoCollapseTimer) clearTimeout(autoCollapseTimer)
    open.value = true
    autoCollapseTimer = setTimeout(() => { open.value = false }, 2000)
  },
  { immediate: true }
)

const isTerminal = computed(() => props.toolCall.name === 'terminal')
const boardSummary = computed(() => taskBoardToolSummary(props.toolCall.result))

const displayLabel = computed(() => props.toolCall.displayLabel?.trim() || props.toolCall.name)
const displaySummary = computed(() => {
  const s = props.toolCall.displaySummary?.trim()
  if (s) return s
  if (showResults.value) return boardSummary.value ?? ''
  return ''
})
const showResults = computed(() => props.showToolCallResults === true)

const terminalCommand = computed(() => {
  if (!isTerminal.value) return ''
  const text = props.toolCall.arguments?.trim()
  if (!text) return ''
  try {
    const parsed = JSON.parse(text)
    return parsed.command || ''
  } catch { return text }
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

const terminalMeta = computed(() => {
  const result = terminalResult.value
  if (!result) return ''
  const items = []
  if (typeof result.exitCode !== 'undefined' && result.exitCode !== null) items.push(`exit ${result.exitCode}`)
  if (result.timedOut) items.push('timeout')
  if (result.runAborted) items.push('已结束命令')
  if (result.cancelled) items.push('已停止')
  return items.join(' · ')
})

/** Legacy rows: status success but result JSON indicates timeout or non-zero exit. */
const effectiveStatus = computed(() => {
  if (!isTerminal.value || props.toolCall.status !== 'success') return props.toolCall.status
  const r = terminalResult.value
  if (r?.timedOut === true) return 'failed' as ToolCall['status']
  if (r?.runAborted === true || r?.cancelled === true) return 'failed' as ToolCall['status']
  const code = r?.exitCode
  if (typeof code === 'number' && code !== 0) return 'failed' as ToolCall['status']
  return props.toolCall.status
})

const statusInfo = computed(() => {
  switch (effectiveStatus.value) {
    case 'pending_approval': return { label: '等待确认', color: 'text-warning' }
    case 'running': return { label: '执行中', color: 'text-accent' }
    case 'success': return { label: '成功', color: 'text-success' }
    case 'failed': return { label: '失败', color: 'text-danger' }
    case 'rejected': return { label: '已拒绝', color: 'text-slate-400' }
  }
  return { label: '', color: '' }
})

function approve(ok: boolean) {
  chat.approve(props.toolCall, ok)
}

function abortTerminalOnly() {
  chat.abortTerminalOnly()
}
</script>

<template>
  <div class="rounded-xl border border-border panel overflow-hidden">
    <button
      class="w-full px-3 py-2 flex items-center gap-2 text-xs hover:bg-hover transition cursor-pointer"
      @click="open = !open"
    >
      <component :is="open ? ChevronDown : ChevronRight" class="w-3.5 h-3.5 text-slate-400" />
      <Wrench class="w-3.5 h-3.5 text-accent" />
      <span class="font-medium text-foreground truncate">{{ displayLabel }}</span>
      <span v-if="displaySummary" class="text-[10px] text-muted truncate">· {{ displaySummary }}</span>
      <span v-if="toolCall.riskLevel === 'high'" class="ml-1 px-1.5 py-0.5 rounded text-[10px] bg-danger/20 text-danger flex items-center gap-1">
        <ShieldAlert class="w-3 h-3" />高风险
      </span>
      <span class="ml-auto flex items-center gap-1.5" :class="statusInfo.color">
        <Loader2 v-if="effectiveStatus === 'running'" class="w-3 h-3 animate-spin" />
        <CheckCircle2 v-else-if="effectiveStatus === 'success'" class="w-3 h-3" />
        <XCircle v-else-if="effectiveStatus === 'failed' || effectiveStatus === 'rejected'" class="w-3 h-3" />
        <span class="text-[11px]">{{ statusInfo.label }}</span>
        <span v-if="toolCall.durationMs" class="text-slate-500 text-[10px]">{{ toolCall.durationMs }}ms</span>
      </span>
    </button>

    <div v-if="open" class="px-3 pb-3 space-y-2">
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
          <pre class="text-[12px] bg-black/60 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-green-400 font-mono">{{ terminalCommand || '—' }}</pre>
        </div>
        <div v-if="showResults && (toolCall.result || toolCall.terminalOutput)">
          <div class="flex items-center justify-between text-[10px] uppercase tracking-wider text-slate-500 mb-1">
            <span>控制台输出</span>
            <span v-if="terminalMeta" class="normal-case tracking-normal">{{ terminalMeta }}</span>
          </div>
          <pre class="text-[12px] bg-black/60 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-slate-200 max-h-64">{{ terminalOutput || '—' }}</pre>
        </div>
      </template>

      <template v-else>
        <div>
          <div class="text-[10px] uppercase tracking-wider text-slate-500 mb-1">参数</div>
          <pre class="text-[12px] bg-black/40 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-slate-200">{{ prettyArgs || '—' }}</pre>
        </div>

        <div v-if="showResults && toolCall.result">
          <div class="text-[10px] uppercase tracking-wider text-slate-500 mb-1">结果</div>
          <pre class="text-[12px] bg-black/40 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-slate-200 max-h-48">{{ toolCall.result }}</pre>
        </div>
      </template>

      <div v-if="toolCall.error" class="text-[12px] text-danger">{{ toolCall.error }}</div>

      <p v-if="open" class="text-[10px] text-muted font-mono truncate">{{ toolCall.name }}</p>

      <div v-if="toolCall.status === 'pending_approval'" class="flex items-center gap-2 pt-1">
        <button
          class="h-8 px-3 rounded-lg bg-success/20 hover:bg-success/30 text-success text-xs flex items-center gap-1.5 cursor-pointer transition"
          @click="approve(true)"
        ><Check class="w-3.5 h-3.5" />允许</button>
        <button
          class="h-8 px-3 rounded-lg bg-danger/15 hover:bg-danger/25 text-danger text-xs flex items-center gap-1.5 cursor-pointer transition"
          @click="approve(false)"
        ><X class="w-3.5 h-3.5" />拒绝</button>
      </div>
    </div>
  </div>
</template>
