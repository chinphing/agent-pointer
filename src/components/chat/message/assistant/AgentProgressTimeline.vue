<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  CheckCircle2,
  XCircle,
  Loader2,
  Sparkles,
  ListTodo,
  ChevronDown,
  ChevronRight
} from 'lucide-vue-next'
import type { AgentTrace, SupervisorPlanTask } from '../../../../types/chat'

const props = defineProps<{
  agentTrace?: AgentTrace[]
  planTasks?: SupervisorPlanTask[]
  isStreaming?: boolean
}>()

const expandedContent = ref<Record<string, boolean>>({})

function tracePaddingLeft(agent: AgentTrace, idx: number): string {
  let d = agent.depth
  if (d == null) d = idx === 0 ? 0 : 1
  const px = 8 + Math.min(d, 8) * 14
  return `${px}px`
}

function statusIcon(status: string) {
  const s = status.toLowerCase()
  if (s === 'completed' || s === 'done' || s === 'success') return CheckCircle2
  if (s === 'failed' || s === 'error') return XCircle
  if (s === 'planning' || s === 'summarizing') return Sparkles
  if (s === 'running') return Loader2
  return ListTodo
}

function statusClass(status: string): string {
  const s = status.toLowerCase()
  if (s === 'completed' || s === 'done') return 'text-success'
  if (s === 'failed') return 'text-danger'
  if (s === 'running') return 'text-accent animate-spin'
  if (s === 'planning' || s === 'summarizing') return 'text-accent'
  return 'text-muted'
}

function statusLabel(status: string): string {
  const map: Record<string, string> = {
    planning: '规划中',
    running: '执行中',
    completed: '已完成',
    failed: '失败',
    summarizing: '整合结果'
  }
  return map[status.toLowerCase()] ?? status
}

const leadTrace = computed(() => props.agentTrace?.find(a => (a.depth ?? 0) === 0))
const childTraces = computed(() => props.agentTrace?.filter(a => (a.depth ?? 0) > 0) ?? [])

const progressSummary = computed(() => {
  const children = childTraces.value
  if (!children.length) return ''
  const done = children.filter(c => ['completed', 'done'].includes(c.status.toLowerCase())).length
  return `${done}/${children.length}`
})

function toggleContent(id: string) {
  expandedContent.value[id] = !expandedContent.value[id]
}
</script>

<template>
  <div
    v-if="agentTrace?.length || planTasks?.length"
    class="rounded-lg border border-border bg-accent-muted/30 overflow-hidden text-[12px]"
  >
    <div v-if="leadTrace" class="px-3 py-2 border-b border-border flex items-center gap-2">
      <component :is="statusIcon(leadTrace.status)" class="w-3.5 h-3.5 shrink-0" :class="statusClass(leadTrace.status)" />
      <span class="font-medium text-foreground">{{ leadTrace.name }}</span>
      <span class="text-muted">{{ statusLabel(leadTrace.status) }}</span>
      <span v-if="progressSummary" class="text-muted ml-auto">{{ progressSummary }}</span>
      <span v-if="leadTrace.detail" class="text-muted truncate max-w-[50%]">{{ leadTrace.detail }}</span>
    </div>

    <div v-if="planTasks?.length" class="px-3 py-2 border-b border-border space-y-1">
      <div class="text-[11px] text-muted font-medium">计划 {{ planTasks.length }} 项</div>
      <div
        v-for="t in planTasks"
        :key="t.id"
        class="flex items-center gap-2 text-foreground/90"
      >
        <ListTodo class="w-3 h-3 text-muted shrink-0" />
        <span class="truncate">{{ t.title || t.id }}</span>
        <span class="text-muted text-[10px] shrink-0">{{ t.agentId }}</span>
      </div>
    </div>

    <div v-if="childTraces.length" class="px-2 py-2 space-y-1.5">
      <div
        v-for="(agent, idx) in childTraces"
        :key="agent.id + '-' + idx + '-' + (agent.status ?? '')"
        class="rounded-md bg-card border border-border p-2"
        :style="{ marginLeft: tracePaddingLeft(agent, idx) }"
      >
        <div class="flex flex-wrap items-center gap-2">
          <component :is="statusIcon(agent.status)" class="w-3 h-3 shrink-0" :class="statusClass(agent.status)" />
          <span class="text-foreground font-medium">{{ agent.name }}</span>
          <span class="text-muted">{{ statusLabel(agent.status) }}</span>
          <span v-if="agent.detail" class="text-muted truncate flex-1 min-w-0">{{ agent.detail }}</span>
          <span
            v-if="agent.status === 'running' && agent.content"
            class="text-muted text-[10px]"
          >输出中… {{ agent.content.length }} 字</span>
        </div>
        <button
          v-if="agent.content && agent.content.length > 80"
          type="button"
          class="mt-1 flex items-center gap-1 text-[10px] text-accent hover:underline"
          @click="toggleContent(agent.id)"
        >
          <ChevronRight v-if="!expandedContent[agent.id]" class="w-3 h-3" />
          <ChevronDown v-else class="w-3 h-3" />
          {{ expandedContent[agent.id] ? '收起' : '查看输出' }}
        </button>
        <pre
          v-if="agent.content && (expandedContent[agent.id] || agent.content.length <= 80)"
          class="mt-1 max-h-36 overflow-auto whitespace-pre-wrap rounded bg-code-bg/80 p-2 text-[11px] text-muted leading-relaxed border border-border"
        >{{ agent.content }}</pre>
      </div>
    </div>
  </div>
</template>
