<script setup lang="ts">
import { computed, inject, ref, watch, type Ref } from 'vue'
import { Code } from 'lucide-vue-next'
import type { AgentTrace, ChatMessage, TaskBoardDocument, ToolCall } from '../../../../types/chat'
import { traceAgentLabel, type ResolvedAgentUi } from '../../../../lib/agentUi'
import { buildCompressionProgressLabel } from '../../../../lib/compressionMessage'
import {
  emptySubAgentToolStats,
  resolveCollapsedSubAgentView,
  subAgentIdFromTraceId
} from '../../../../lib/subAgentStats'
import {
  buildToolRawArgsFromMessages,
  computeSubAgentStatsFromMessages,
  latestSubAgentBodyModelFromScoped,
  scopedAssistantMessagesForTrace,
  scopedMessagesForTrace,
  subAgentFrameOwnsCompression
} from '../../../../lib/subAgentMessages'
import { useChatStore } from '../../../../stores/chat'
import {
  isSubResponseToolName,
  isSubTraceUiCollapsed,
  toggleSubTraceExpanded
} from '../../../../lib/subAgentSession'
import {
  compactToolCallLiveText,
  compactToolCallStatusLine,
  effectiveToolDisplaySummary,
  isToolCallInProgress,
  latestToolCallForCompactStatus
} from '../../../../lib/toolCallDisplay'
import {
  thinkingLabel,
  streamedCharCountFromBody,
  subAgentThinkingActive
} from '../../../../lib/thinkingIndicator'
import { visibleToolCalls } from '../../../../lib/messageTooling'
import { useSettingsStore } from '../../../../stores/settings'
import { useAgentsCatalog } from '../../../../composables/useAgentUi'
import type { AgentMessageBodyModel } from './AgentMessageBody.vue'
import ContextCompressingMarker from '../ContextCompressingMarker.vue'
import RawWirePanel from './RawWirePanel.vue'
import TaskBoardPanel from '../../TaskBoardPanel.vue'
import CollapsedRunHeader from '../../CollapsedRunHeader.vue'
import ToolCallRow from '../../ToolCallRow.vue'

export type SubAgentTaskBoardBinding = {
  document: TaskBoardDocument
  isActive: boolean
  conversationId: string | null
  taskId: string
}

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
  /** Child task board always shown above the process UI (collapsed or expanded). */
  taskBoard?: SubAgentTaskBoardBinding | null
  /** Parent `run_subagent` row; host expands to args. This frame is stats + inner tools. */
  hostTool?: ToolCall | null
}>()

const settingsStore = useSettingsStore()
const agentsCatalog = useAgentsCatalog()
const chatStore = useChatStore()
const traceLabel = computed(() =>
  traceAgentLabel(props.trace, agentsCatalog.value, settingsStore.settings)
)
const rawContentViewEnabled = computed(() => settingsStore.settings.rawContentViewEnabled === true)

/** Prefer nested trace anchor when present; otherwise the lead message id. */
const effectiveAnchorId = computed(
  () => props.trace.anchorMessageId?.trim() || props.anchorMessageId
)

const scopedMessages = computed(() =>
  scopedAssistantMessagesForTrace(
    props.messages,
    effectiveAnchorId.value,
    props.trace.id,
    props.trace.agentInstanceId
  )
)

const scopedTraceMessages = computed(() =>
  scopedMessagesForTrace(
    props.messages,
    effectiveAnchorId.value,
    props.trace.id,
    props.trace.agentInstanceId
  )
)

const legacySession = computed(() => props.trace.session)

const isRunning = computed(() => props.trace.status === 'running')

const ownsCompression = computed(() =>
  subAgentFrameOwnsCompression(chatStore.contextCompressing, {
    messages: props.messages,
    anchorMessageId: effectiveAnchorId.value,
    traceId: props.trace.id,
    agentInstanceId: props.trace.agentInstanceId
  })
)

const compressionProgressLabel = computed(() => {
  if (!ownsCompression.value) return ''
  return buildCompressionProgressLabel({
    ...chatStore.contextCompressing,
    inSubAgentFrame: true
  })
})

const collapsed = computed(() => isSubTraceUiCollapsed(props.trace))

const latestStreamBody = computed((): AgentMessageBodyModel | null => {
  const scoped = latestSubAgentBodyModelFromScoped(
    props.messages,
    effectiveAnchorId.value,
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

const searchToolCallIds = inject<Ref<string[]>>(
  'currentConversationSearchToolCallIds',
  ref<string[]>([])
)
const activeSearchToolCallId = inject<Ref<string | null>>(
  'currentConversationActiveToolCallId',
  ref<string | null>(null)
)

const goalLabel = computed(() => {
  const host = props.hostTool
  if (!host) return ''
  return effectiveToolDisplaySummary(host)
})

const innerToolCalls = computed((): ToolCall[] => {
  const seen = new Set<string>()
  const out: ToolCall[] = []
  for (const msg of scopedMessages.value) {
    for (const tc of msg.toolCalls ?? []) {
      if (seen.has(tc.id)) continue
      seen.add(tc.id)
      out.push(tc)
    }
  }
  if (out.length > 0) return out
  return latestStreamBody.value?.toolCalls
    ?? legacySession.value?.toolCalls
    ?? []
})

const processInnerTools = computed((): ToolCall[] => {
  if (!props.messageUi.showToolCalls) return []
  return visibleToolCalls(
    innerToolCalls.value.filter(tc => !isSubResponseToolName(tc.name)),
    props.messageUi.hideToolNames,
    props.messageUi.showSidecarToolCalls === true,
    props.messageUi.showNonSidecarToolCalls !== false
  )
})

const liveInnerTool = computed(() => {
  if (!isRunning.value) return null
  const latest = latestToolCallForCompactStatus(processInnerTools.value)
  if (!latest || !isToolCallInProgress(latest.status)) return null
  return latest
})

const showThinkingInSummary = computed(() =>
  subAgentThinkingActive({
    running: isRunning.value,
    hasInProgressTool:
      !!liveInnerTool.value || scopedHasInProgressTools(scopedTraceMessages.value)
  })
)

const visibleInnerTools = computed((): ToolCall[] => {
  const tools = processInnerTools.value
  const live = liveInnerTool.value
  if (!live) return tools
  if (tools.some(tc => tc.id === live.id)) return tools
  return [...tools, live]
})

const collapsedView = computed(() => {
  const stats =
    scopedTraceMessages.value.length > 0
      ? computeSubAgentStatsFromMessages(scopedTraceMessages.value)
      : (legacySession.value?.stats ?? emptySubAgentToolStats())
  const thinking = showThinkingInSummary.value
    ? thinkingLabel(latestStreamBody.value ? streamedCharCountFromBody(latestStreamBody.value) : 0)
    : null
  const live = liveInnerTool.value
    ? compactToolCallLiveText(liveInnerTool.value, chatStore.current?.workspaceRoot)
    : null
  return resolveCollapsedSubAgentView({
    orphanTitle: props.hostTool ? '' : (goalLabel.value || traceLabel.value),
    status: props.trace.status,
    stats,
    agentId: subAgentIdFromTraceId(props.trace.id),
    liveToolLine: live,
    thinkingLine: thinking
  })
})

const summaryLine = computed(() => collapsedView.value.summaryLine)
const liveLine = computed(() => (collapsed.value ? collapsedView.value.liveLine : null))
const liveToolName = computed(() => {
  if (!collapsed.value) return null
  return liveInnerTool.value?.name ?? null
})
const liveAriaLabel = computed(() => {
  const summary = summaryLine.value.trim()
  if (summary) return summary
  if (liveInnerTool.value) {
    return compactToolCallStatusLine(liveInnerTool.value, chatStore.current?.workspaceRoot, {
      includeStatus: false
    })
  }
  return liveLine.value || '子任务过程'
})
const liveKey = computed(() => {
  if (!collapsed.value || !isRunning.value) return null
  if (liveInnerTool.value?.id) return liveInnerTool.value.id
  if (liveLine.value?.trim()) return 'thinking'
  return null
})

const showProcessHeader = computed(() =>
  !!summaryLine.value.trim()
  || !!(collapsed.value && (liveLine.value?.trim() || isRunning.value))
  || (!collapsed.value && visibleInnerTools.value.length > 0)
)

const showCompressionMarker = computed(
  () => ownsCompression.value && !!compressionProgressLabel.value
)

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
  if (visibleInnerTools.value.length === 0) return
  toggleSubTraceExpanded(props.trace)
}

const isSearchHit = computed(() => {
  const ids = searchToolCallIds.value
  if (ids.length === 0) return false
  return visibleInnerTools.value.some(tc => ids.includes(tc.id))
})

watch(
  isSearchHit,
  hit => {
    if (hit && isSubTraceUiCollapsed(props.trace)) {
      toggleSubTraceExpanded(props.trace)
    }
  },
  { immediate: true }
)
</script>

<template>
  <div
    class="sub-agent-frame min-w-0 w-full overflow-hidden"
    :style="{
      marginLeft: `${Math.max(0, (trace.depth ?? 1) - 1) * 12}px`
    }"
  >
    <div class="sub-agent-nested min-w-0 w-full space-y-0.5">
    <!-- Always above process UI (collapsed summary or expanded tool cards). -->
    <div
      v-if="taskBoard"
      class="flex justify-start min-w-0 max-w-full"
    >
      <TaskBoardPanel
        :document="taskBoard.document"
        :is-active="taskBoard.isActive"
        :conversation-id="taskBoard.conversationId"
        :task-id="taskBoard.taskId"
        :agent-status="trace.status"
      />
    </div>

    <div
      v-if="showProcessHeader"
      class="flex items-start gap-2 min-w-0"
    >
      <CollapsedRunHeader
        :summary-line="summaryLine"
        :live-line="liveLine"
        :live-tool-name="liveToolName"
        :live-key="liveKey"
        :expanded="!collapsed"
        :failed="trace.status === 'failed'"
        :force-live-slot="collapsed && isRunning"
        :show-chevron="visibleInnerTools.length > 0"
        :live-busy="collapsed && !!liveInnerTool"
        :aria-label="liveAriaLabel"
        @toggle="toggleExpanded"
      />
      <button
        v-if="!collapsed && hasRawWire"
        type="button"
        class="message-action-btn shrink-0"
        :class="showRawWire ? 'text-foreground' : 'text-muted hover:text-foreground'"
        :title="showRawWire ? '隐藏原始内容' : '查看原始内容'"
        @click.stop="showRawWire = !showRawWire"
      >
        <Code class="w-3.5 h-3.5" />
      </button>
    </div>

    <div
      v-if="!collapsed"
      class="space-y-0.5"
    >
      <ContextCompressingMarker
        v-if="showCompressionMarker"
        :label="compressionProgressLabel"
      />
      <ToolCallRow
        v-for="tc in visibleInnerTools"
        :key="tc.id"
        :tool-call="tc"
        :show-tool-call-results="messageUi.showToolCallResults"
        :is-search-match="searchToolCallIds.includes(tc.id)"
        :is-active-search-match="activeSearchToolCallId === tc.id"
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
  </div>
</template>
