<script setup lang="ts">
import { computed, inject, ref, watch, type Ref } from 'vue'
import { Code } from 'lucide-vue-next'
import type { AgentTrace, ChatMessage, TaskBoardDocument, ToolCall } from '../../../../types/chat'
import { traceAgentLabel, type ResolvedAgentUi } from '../../../../lib/agentUi'
import { buildCompressionProgressLabel } from '../../../../lib/compressionMessage'
import {
  emptySubAgentToolStats,
  resolveCollapsedSubAgentView,
  resolveTraceAgentId,
  SUB_AGENT_PROCESS_PLACEHOLDER
} from '../../../../lib/subAgentStats'
import { useConversationScopedStore } from '../../../../lib/conversationScoped'
import {
  buildToolRawArgsFromMessages,
  computeSubAgentStatsFromMessages,
  latestSubAgentBodyModelFromSpawnRows,
  subAgentFrameOwnsCompression
} from '../../../../lib/subAgentMessages'
import { useChatStore } from '../../../../stores/chat'
import {
  isSubAgentTraceTerminal,
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
  thinkingCharCountForCollapsedSubAgent,
  subAgentThinkingActive
} from '../../../../lib/thinkingIndicator'
import { isInteractiveToolCall, visibleToolCalls } from '../../../../lib/messageTooling'
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

const legacySession = computed(() => props.trace.session)

const isRunning = computed(() => props.trace.status === 'running')

const collapsed = computed(() => isSubTraceUiCollapsed(props.trace))

const scopedMessages = computed(() => {
  // Queued / not-yet-running collapsed frames have no scoped rows yet — skip O(n) scans.
  if (collapsed.value && !isRunning.value && !legacySession.value) {
    if (!isSubAgentTraceTerminal(props.trace.status)) return []
  }
  return chatStore
    .scopedMessagesForTraceCached(
      effectiveAnchorId.value,
      props.trace.id,
      props.trace.agentInstanceId
    )
    .filter(m => m.role === 'assistant')
    .sort((a, b) => a.createdAt - b.createdAt)
})

const scopedTraceMessages = computed(() => {
  if (collapsed.value && !isRunning.value && !legacySession.value) {
    if (!isSubAgentTraceTerminal(props.trace.status)) return []
  }
  return chatStore.scopedMessagesForTraceCached(
    effectiveAnchorId.value,
    props.trace.id,
    props.trace.agentInstanceId
  )
})

const ownsCompression = computed(() => {
  if (!isRunning.value) return false
  const compressing = chatStore.contextCompressing
  const cut = compressing?.insertBeforeMessageId?.trim() ?? ''
  const convId = chatStore.currentId
  const cutScopedRow =
    cut && convId ? useConversationScopedStore().findRow(convId, cut) : undefined
  return subAgentFrameOwnsCompression(compressing, {
    messages: scopedTraceMessages.value,
    cutScopedRow,
    anchorMessageId: effectiveAnchorId.value,
    traceId: props.trace.id,
    agentInstanceId: props.trace.agentInstanceId
  })
})

const compressionProgressLabel = computed(() => {
  if (!ownsCompression.value) return ''
  return buildCompressionProgressLabel({
    ...chatStore.contextCompressing,
    inSubAgentFrame: true
  })
})

const latestStreamBody = computed((): AgentMessageBodyModel | null => {
  if (collapsed.value && !isRunning.value && scopedMessages.value.length === 0 && !legacySession.value) {
    return null
  }
  const scoped = latestSubAgentBodyModelFromSpawnRows(
    scopedTraceMessages.value,
    props.trace.status
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
  const pushAll = (list: ToolCall[] | undefined) => {
    for (const tc of list ?? []) {
      if (seen.has(tc.id)) continue
      seen.add(tc.id)
      out.push(tc)
    }
  }
  for (const msg of scopedMessages.value) {
    pushAll(msg.toolCalls)
  }
  // Merge legacy/session tools too — scoped rows alone can miss an in-flight
  // ask_user that only landed on trace.session (or the reverse).
  pushAll(latestStreamBody.value?.toolCalls ?? legacySession.value?.toolCalls)
  return out
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

const interactiveInnerTools = computed((): ToolCall[] =>
  processInnerTools.value.filter(isInteractiveToolCall)
)

const liveInnerTool = computed(() => {
  if (!isRunning.value) return null
  // Collapsed frames surface ask_user / approval cards below the summary —
  // keep the live line for non-interactive process tools only.
  const candidates = collapsed.value
    ? processInnerTools.value.filter(tc => !isInteractiveToolCall(tc))
    : processInnerTools.value
  const latest = latestToolCallForCompactStatus(candidates)
  if (!latest || !isToolCallInProgress(latest.status)) return null
  return latest
})

const showThinkingInSummary = computed(() => {
  // Live-signal edge: hidden sidecar tool transitions don't change
  // liveInnerTool identity, so without this the dots stall.
  void chatStore.getSubAgentLiveSignal(
    props.trace.agentInstanceId?.trim() || props.trace.id
  )
  // Guard 1: conversation no longer generating — run ended or was cancelled.
  const convId = chatStore.currentId
  if (convId && !chatStore.isConversationGenerating(convId)) return false
  // Guard 2: newer messages exist after this frame's anchor — thinking is stale.
  const anchorIdx = props.messages.findIndex(m => m.id === props.anchorMessageId)
  if (anchorIdx >= 0 && props.messages.length > anchorIdx + 1) {
    const hasNewerUserMessage = props.messages.slice(anchorIdx + 1).some(
      m => m.role === 'user' && !m.anchorMessageId
    )
    if (hasNewerUserMessage) return false
  }
  return subAgentThinkingActive({
    running: isRunning.value,
    hasInProgressTool:
      !!liveInnerTool.value || scopedHasInProgressTools(scopedTraceMessages.value)
  })
})

const visibleInnerTools = computed((): ToolCall[] => {
  const tools = processInnerTools.value
  const live = liveInnerTool.value
  if (!live) return tools
  if (tools.some(tc => tc.id === live.id)) return tools
  return [...tools, live]
})

const collapsedView = computed(() => {
  // Read the live signal directly so this computed re-evaluates when the
  // fingerprint changes — getRows() returns the same array reference, so
  // scopedTraceMessages alone won't trigger a recompute.
  const _live = chatStore.getSubAgentLiveSignal(
    props.trace.agentInstanceId?.trim() || props.trace.id
  )
  void _live
  const persisted = props.trace.summaryLine?.trim() || ''
  const stats =
    scopedTraceMessages.value.length > 0
      ? computeSubAgentStatsFromMessages(scopedTraceMessages.value)
      : (legacySession.value?.stats ?? emptySubAgentToolStats())
  const thinking = showThinkingInSummary.value
    ? thinkingLabel(
        thinkingCharCountForCollapsedSubAgent(
          scopedTraceMessages.value,
          legacySession.value ?? null
        )
      )
    : null
  const live = liveInnerTool.value
    ? compactToolCallLiveText(liveInnerTool.value, chatStore.current?.workspaceRoot)
    : null
  const view = resolveCollapsedSubAgentView({
    orphanTitle: props.hostTool ? '' : (goalLabel.value || traceLabel.value),
    status: props.trace.status,
    stats,
    agentId: resolveTraceAgentId(props.trace),
    liveToolLine: live,
    thinkingLine: thinking
  })
  if (!view.summaryLine.trim() && persisted && scopedTraceMessages.value.length === 0) {
    return { summaryLine: persisted, liveLine: view.liveLine }
  }
  return view
})

const summaryLine = computed(
  () => collapsedView.value.summaryLine.trim() || SUB_AGENT_PROCESS_PLACEHOLDER
)
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
  toggleSubTraceExpanded(props.trace)
}

watch(
  () => !collapsed.value,
  open => {
    if (!open) return
    void chatStore.ensureScopedMessagesForTrace(
      effectiveAnchorId.value,
      props.trace.id,
      props.trace.agentInstanceId
    )
  }
)

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

    <div class="flex items-start gap-2 min-w-0">
      <CollapsedRunHeader
        :summary-line="summaryLine"
        :live-line="liveLine"
        :live-tool-name="liveToolName"
        :live-key="liveKey"
        :expanded="!collapsed"
        :failed="trace.status === 'failed'"
        :force-live-slot="collapsed && isRunning"
        :show-chevron="true"
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
    <!-- Collapsed: keep pending ask_user / approval cards actionable (main-turn parity). -->
    <div
      v-else-if="interactiveInnerTools.length > 0"
      class="space-y-0.5"
    >
      <ToolCallRow
        v-for="tc in interactiveInnerTools"
        :key="tc.id"
        :tool-call="tc"
        :show-tool-call-results="messageUi.showToolCallResults"
        :is-search-match="searchToolCallIds.includes(tc.id)"
        :is-active-search-match="activeSearchToolCallId === tc.id"
      />
    </div>
    </div>
  </div>
</template>
