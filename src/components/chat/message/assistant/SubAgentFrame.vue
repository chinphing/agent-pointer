<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ChevronDown, ChevronRight, Code } from 'lucide-vue-next'
import type { AgentTrace, ChatMessage } from '../../../../types/chat'
import { traceAgentLabel, type ResolvedAgentUi } from '../../../../lib/agentUi'
import {
  formatSubAgentSummaryLine,
  subAgentIdFromTraceId,
  subAgentStatusLabel
} from '../../../../lib/subAgentStats'
import {
  buildSubAgentBodyModelsFromScoped,
  buildToolRawArgsFromMessages,
  computeSubAgentStatsFromMessages,
  latestSubAgentBodyModelFromScoped,
  scopedAssistantMessagesForTrace,
  scopedMessagesForTrace,
  subTraceHasVisibleActivityFromMessages
} from '../../../../lib/subAgentMessages'
import {
  subTraceHasVisibleActivity,
  isSubTraceUiCollapsed,
  toggleSubTraceExpanded
} from '../../../../lib/subAgentSession'
import { thinkingLabel, streamedCharCountFromBody } from '../../../../lib/thinkingIndicator'
import { useSettingsStore } from '../../../../stores/settings'
import { useAgentsCatalog } from '../../../../composables/useAgentUi'
import AgentMessageBody, { type AgentMessageBodyModel } from './AgentMessageBody.vue'
import RawWirePanel from './RawWirePanel.vue'

const props = defineProps<{
  trace: AgentTrace
  anchorMessageId: string
  messages: ChatMessage[]
  messageUi: ResolvedAgentUi
  createdAt: number
  thoughtsDebugEnabled?: boolean
  generating: boolean
  isActiveGenerationMessage: boolean
  showMessageActions?: boolean
}>()

const settingsStore = useSettingsStore()
const agentsCatalog = useAgentsCatalog()
const traceLabel = computed(() =>
  traceAgentLabel(props.trace, agentsCatalog.value, settingsStore.settings)
)
const rawContentViewEnabled = computed(() => settingsStore.settings.rawContentViewEnabled === true)

const scopedMessages = computed(() =>
  scopedAssistantMessagesForTrace(
    props.messages,
    props.anchorMessageId,
    props.trace.id,
    props.trace.agentInstanceId
  )
)

const scopedTraceMessages = computed(() =>
  scopedMessagesForTrace(
    props.messages,
    props.trace.anchorMessageId
      ? props.trace.anchorMessageId
      : props.anchorMessageId,
    props.trace.id,
    props.trace.agentInstanceId
  )
)

const legacySession = computed(() => props.trace.session)

const isRunning = computed(() => props.trace.status === 'running')

const collapsed = computed(() => isSubTraceUiCollapsed(props.trace))

const hasVisibleActivity = computed(() => {
  if (scopedMessages.value.length > 0) {
    return subTraceHasVisibleActivityFromMessages(scopedMessages.value)
  }
  return subTraceHasVisibleActivity(props.trace)
})

const latestStreamBody = computed((): AgentMessageBodyModel | null => {
  const scoped = latestSubAgentBodyModelFromScoped(
    props.messages,
    props.anchorMessageId,
    props.trace.id,
    props.trace.status,
    props.trace.agentInstanceId
  )
  if (scoped) return scoped
  const s = legacySession.value
  if (!s) return null
  return {
    thoughts: s.thoughts,
    toolNamePreview: s.toolNamePreview,
    responseTextDraft: s.responseTextDraft,
    reasoning: s.reasoning,
    rawContent: s.rawContent,
    content: undefined,
    contentStreaming: s.contentStreaming === true,
    toolCalls: s.toolCalls,
    status: props.trace.status === 'failed' ? 'error' : isRunning.value ? 'streaming' : 'done',
    createdAt: props.createdAt
  }
})

function scopedHasInProgressTools(messages: ChatMessage[]): boolean {
  for (const msg of messages.filter(m => m.role === 'assistant')) {
    for (const tc of msg.toolCalls ?? []) {
      if (tc.status === 'running' || tc.status === 'pending' || tc.status === 'pending_approval') {
        return true
      }
    }
  }
  return false
}

const showThinkingInSummary = computed(() => {
  if (!isRunning.value) return false
  if (scopedHasInProgressTools(scopedTraceMessages.value)) return false
  const body = latestStreamBody.value
  if (body?.toolNamePreview?.trim()) return false
  if ((body?.toolCalls?.length ?? 0) > 0) return false
  const streaming =
    body?.status === 'streaming'
    || body?.contentStreaming === true
  if (streaming) return true
  return props.generating && props.isActiveGenerationMessage
})

const summaryLine = computed(() => {
  if (showThinkingInSummary.value) {
    const chars = latestStreamBody.value ? streamedCharCountFromBody(latestStreamBody.value) : 0
    return `${traceLabel.value} · ${thinkingLabel(chars)}`
  }
  if (isRunning.value && !hasVisibleActivity.value) {
    return `${traceLabel.value} · ${subAgentStatusLabel(props.trace.status)}…`
  }
  const stats =
    scopedTraceMessages.value.length > 0
      ? computeSubAgentStatsFromMessages(scopedTraceMessages.value)
      : (legacySession.value?.stats ?? { searchCount: 0, readCount: 0 })
  return formatSubAgentSummaryLine(
    traceLabel.value,
    props.trace.status,
    stats,
    subAgentIdFromTraceId(props.trace.id)
  )
})

const bodyModels = computed((): AgentMessageBodyModel[] => {
  const scoped = buildSubAgentBodyModelsFromScoped(
    props.messages,
    props.anchorMessageId,
    props.trace.id,
    props.trace.status,
    props.trace.agentInstanceId
  )
  if (scoped.length > 0) return scoped
  const s = legacySession.value
  if (!s) return []
  return [
    {
      thoughts: s.thoughts,
      toolNamePreview: s.toolNamePreview,
      responseTextDraft: s.responseTextDraft,
      reasoning: s.reasoning,
      rawContent: s.rawContent,
      content: undefined,
      contentStreaming: s.contentStreaming === true,
      toolCalls: s.toolCalls,
      status: props.trace.status === 'failed' ? 'error' : isRunning.value ? 'streaming' : 'done',
      createdAt: props.createdAt,
      errorMessage: props.trace.status === 'failed' ? props.trace.detail : undefined
    }
  ]
})

const subFrameActive = computed(
  () => props.generating && props.isActiveGenerationMessage && isRunning.value
)

const activeBodyIndex = computed(() => Math.max(0, bodyModels.value.length - 1))

const toolRawArgs = computed(() => {
  if (scopedMessages.value.length > 0) {
    return buildToolRawArgsFromMessages(scopedMessages.value)
  }
  const calls = legacySession.value?.toolCalls
  if (!calls?.length) return ''
  return buildToolRawArgsFromMessages([{ id: '', role: 'assistant', content: '', status: 'done', createdAt: 0, toolCalls: calls }])
})

const hasRawWire = computed(() => {
  if (!rawContentViewEnabled.value) return false
  if (scopedMessages.value.length > 0) {
    const reasoning = scopedMessages.value.some(m => (m.reasoning?.trim() ?? '').length > 0)
    const raw = scopedMessages.value.some(m => (m.rawContent?.trim() ?? '').length > 0)
    return reasoning || raw || toolRawArgs.value.trim().length > 0
  }
  const s = legacySession.value
  const reasoning = s?.reasoning?.trim() ?? ''
  const raw = s?.rawContent?.trim() ?? ''
  return reasoning.length > 0 || raw.length > 0 || toolRawArgs.value.trim().length > 0
})

const rawWireReasoning = computed(() =>
  scopedMessages.value.map(m => m.reasoning?.trim()).filter(Boolean).join('\n\n')
    || legacySession.value?.reasoning
)

const rawWireContent = computed(() =>
  scopedMessages.value.map(m => m.rawContent?.trim()).filter(Boolean).join('\n\n')
    || legacySession.value?.rawContent
)

const showRawWire = ref(false)

watch(rawContentViewEnabled, on => {
  if (!on) showRawWire.value = false
})

function toggleExpanded() {
  toggleSubTraceExpanded(props.trace)
}

const statusClass = computed(() =>
  props.trace.status === 'failed'
    ? 'text-danger'
    : 'text-muted'
)
</script>

<template>
  <div
    class="rounded-xl my-2 overflow-hidden"
    :class="collapsed ? 'py-2 px-3' : 'p-3'"
    :style="{ marginLeft: `${Math.max(0, (trace.depth ?? 1) - 1) * 12}px` }"
  >
    <button
      v-if="collapsed"
      type="button"
      class="w-full flex items-center gap-2 min-w-0 text-left hover:bg-hover/50 rounded-md px-1 py-0.5 transition"
      :aria-expanded="false"
      @click="toggleExpanded"
    >
      <ChevronRight class="w-4 h-4 shrink-0 text-muted" />
      <span class="flex-1 min-w-0 text-[13px] text-foreground truncate">{{ summaryLine }}</span>
    </button>

    <div v-else class="space-y-2">
      <div class="flex items-center gap-2 min-w-0">
        <button
          type="button"
          class="min-w-0 flex-1 flex items-center gap-2 text-left text-[13px] font-medium text-muted hover:text-foreground transition"
          :aria-expanded="true"
          @click="toggleExpanded"
        >
          <ChevronDown class="w-4 h-4 shrink-0 text-muted" />
          <span class="truncate">{{ traceLabel }}</span>
          <span class="text-xs shrink-0" :class="statusClass">{{ trace.status }}</span>
        </button>
        <button
          v-if="hasRawWire"
          type="button"
          class="message-action-btn shrink-0"
          :class="showRawWire ? 'text-foreground' : 'text-muted hover:text-foreground'"
          :title="showRawWire ? '隐藏原始内容' : '查看原始内容'"
          @click.stop="showRawWire = !showRawWire"
        >
          <Code class="w-3.5 h-3.5" />
        </button>
      </div>

      <AgentMessageBody
        v-for="(body, index) in bodyModels"
        :key="`${trace.id}-${index}`"
        :body="body"
        :message-ui="messageUi"
        hide-response
        hide-copy
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :generating="generating"
        :is-active-generation-message="subFrameActive && index === activeBodyIndex"
      />

      <RawWirePanel
        v-if="showRawWire && hasRawWire"
        :reasoning="rawWireReasoning"
        :raw-content="rawWireContent"
        :tool-raw-args="toolRawArgs"
        @close="showRawWire = false"
      />
    </div>
  </div>
</template>
