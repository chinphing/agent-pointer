<script setup lang="ts">
import { computed, ref, watch, onUnmounted } from 'vue'
import { Wrench, ChevronDown, ChevronRight, CheckCircle2, XCircle, Loader2, ShieldAlert, Check, X } from 'lucide-vue-next'
import type { ToolCall } from '../../types/chat'
import { useChatStore } from '../../stores/chat'

const props = defineProps<{ toolCall: ToolCall }>()
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
  return items.join(' · ')
})

const statusInfo = computed(() => {
  switch (props.toolCall.status) {
    case 'pending_approval': return { label: '等待确认', color: 'text-warning' }
    case 'running': return { label: '执行中', color: 'text-primary-cyan' }
    case 'success': return { label: '成功', color: 'text-success' }
    case 'failed': return { label: '失败', color: 'text-danger' }
    case 'rejected': return { label: '已拒绝', color: 'text-slate-400' }
  }
  return { label: '', color: '' }
})

function approve(ok: boolean) {
  chat.approve(props.toolCall, ok)
}
</script>

<template>
  <div class="rounded-xl border border-white/5 glass overflow-hidden">
    <button
      class="w-full px-3 py-2 flex items-center gap-2 text-xs hover:bg-white/[0.04] transition cursor-pointer"
      @click="open = !open"
    >
      <component :is="open ? ChevronDown : ChevronRight" class="w-3.5 h-3.5 text-slate-400" />
      <Wrench class="w-3.5 h-3.5 text-primary-fuchsia" />
      <span class="font-medium text-slate-100">{{ toolCall.name }}</span>
      <span v-if="toolCall.riskLevel === 'high'" class="ml-1 px-1.5 py-0.5 rounded text-[10px] bg-danger/20 text-danger flex items-center gap-1">
        <ShieldAlert class="w-3 h-3" />高风险
      </span>
      <span class="ml-auto flex items-center gap-1.5" :class="statusInfo.color">
        <Loader2 v-if="toolCall.status === 'running'" class="w-3 h-3 animate-spin" />
        <CheckCircle2 v-else-if="toolCall.status === 'success'" class="w-3 h-3" />
        <XCircle v-else-if="toolCall.status === 'failed' || toolCall.status === 'rejected'" class="w-3 h-3" />
        <span class="text-[11px]">{{ statusInfo.label }}</span>
        <span v-if="toolCall.durationMs" class="text-slate-500 text-[10px]">{{ toolCall.durationMs }}ms</span>
      </span>
    </button>

    <div v-if="open" class="px-3 pb-3 space-y-2">
      <template v-if="isTerminal">
        <div>
          <div class="flex items-center justify-between text-[10px] uppercase tracking-wider text-slate-500 mb-1">
            <span>执行命令</span>
          </div>
          <pre class="text-[12px] bg-black/60 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-green-400 font-mono">{{ terminalCommand || '—' }}</pre>
        </div>
        <div v-if="toolCall.result">
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

        <div v-if="toolCall.result">
          <div class="text-[10px] uppercase tracking-wider text-slate-500 mb-1">结果</div>
          <pre class="text-[12px] bg-black/40 rounded-lg p-2.5 border border-white/5 overflow-x-auto text-slate-200 max-h-48">{{ toolCall.result }}</pre>
        </div>
      </template>

      <div v-if="toolCall.error" class="text-[12px] text-danger">{{ toolCall.error }}</div>

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
