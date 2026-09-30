<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

import { computed, inject, ref, watch, type Ref } from 'vue'
import { Code } from 'lucide-vue-next'
import type { AgentTrace, ChatMessage, TaskBoardDocument, ToolCall } from '../../../../types/chat'
import { traceAgentLabel, type ResolvedAgentUi } from '../../../../lib/agentUi'
import { buildCompressionProgressLabel } from '../../../../lib/compressionMessage'
import {
  emptySubAgentToolStats,
  resolveCollapsedSubAgentView,
  resolveSubAgentSummaryDisplay,
  resolveTraceAgentId
} from '../../../../lib/subAgentStats'
import { useConversationScopedStore } from '../../../../lib/conversationScoped'
import {
  buildSubAgentRoundsFromScoped,
  buildToolRawArgsFromMessages,
  computeSubAgentStatsFromMessages,
  latestSubAgentBodyModelFromSpawnRows,
  latestSubAgentContent,
  mergeSubAgentToolCalls,
  subAgentContentPreviewLine,
  subAgentFrameOwnsCompression
} from '../../../../lib/subAgentMessages'
import { traceSubtreeContainsSearchTarget } from '../../../../lib/subAgentSearch'
import { useChatStore } from '../../../../stores/chat'
import {
  isSubAgentTraceTerminal,
  isSubResponseToolName,
  isSubTraceUiCollapsed,
  toggleSubTraceExpanded
} from '../../../../lib/subAgentSession'
import {
  buildSubAgentTraceTree,
  selfForkTraceLabel
} from '../../../../lib/subAgentTraceTree'
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
import { isPendingApprovalToolCall, isPendingAskUserToolCall, visibleToolCalls } from '../../../../lib/messageTooling'
import { useSettingsStore } from '../../../../stores/settings'
import { useAgentsCatalog } from '../../../../composables/useAgentUi'
import type { AgentMessageBodyModel } from './AgentMessageBody.vue'
import ContextCompressingMarker from '../ContextCompressingMarker.vue'
import RawWirePanel from './RawWirePanel.vue'
import TaskBoardPanel from '../../TaskBoardPanel.vue'
import CollapsedRunHeader from '../../CollapsedRunHeader.vue'
import ToolCallRow from '../../ToolCallRow.vue'
import SubAgentContentBlock from './SubAgentContentBlock.vue'
// Mutual import with the frame is intentional: a child goes through the same
// stub-or-frame decision as a lead-level trace (see the child loop below).
import SubAgentFrameHost from './SubAgentFrameHost.vue'

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
  selfForkTraceLabel(
    traceAgentLabel(props.trace, agentsCatalog.value, settingsStore.settings),
    props.trace
  )
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

/**
 * Direct child frames (this worker's own nested spawns). A nested spawn is recorded
 * on the row of *its issuer*, so the deeper levels come from this layer's own scoped
 * rows — the lead message never carries them. This trace is the head of that subtree
 * and sits on the layer above, so it has to be seeded into the merge or every child
 * resolves to a missing parent and drops out. Collapsed parents hide their subtree.
 */
const childTraces = computed((): AgentTrace[] => {
  if (collapsed.value) return []
  const rows = scopedTraceMessages.value
  if (rows.length === 0) return []
  return buildSubAgentTraceTree({
    leadTraces: [props.trace],
    scopedRows: rows
  }).childrenOf(props.trace.id)
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
/** Scoped row ids whose round text matched the conversation search. */
const searchContentIds = inject<Ref<string[]>>(
  'currentConversationSearchContentIds',
  ref<string[]>([])
)
const activeSearchContentId = inject<Ref<string | null>>(
  'currentConversationActiveSearchContentId',
  ref<string | null>(null)
)

const goalLabel = computed(() => {
  const host = props.hostTool
  if (!host) return ''
  return effectiveToolDisplaySummary(host)
})

const innerToolCalls = computed((): ToolCall[] => {
  // Scoped rows and session are not the same object. A scoped copy can exist
  // with empty arguments while the choice payload only landed on session
  // (or the reverse). Keep the copy that can render the option card.
  return mergeSubAgentToolCalls([
    ...scopedMessages.value.map(msg => msg.toolCalls),
    latestStreamBody.value?.toolCalls,
    legacySession.value?.toolCalls
  ])
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

/** Collapsed frames keep approval cards; pending ask_user lives in the top banner. */
const approvalInnerTools = computed((): ToolCall[] =>
  processInnerTools.value.filter(isPendingApprovalToolCall)
)

const liveInnerTool = computed(() => {
  if (!isRunning.value) return null
  // Collapsed frames surface approval cards below the summary; ask_user is the
  // banner's job now. Keep the live line for other process tools.
  const candidates = collapsed.value
    ? processInnerTools.value.filter(
      tc => !isPendingApprovalToolCall(tc) && !isPendingAskUserToolCall(tc)
    )
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
  // Trace status is the source of truth — do **not** gate on lead
  // `generating`. Background `run_subagent` keeps `trace.status=running`
  // after the parent turn returns a job handle; requiring generating left
  // only the「过程」placeholder with no「思考中」/ live tool gap.
  if (!isRunning.value) return false
  return subAgentThinkingActive({
    running: true,
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

/** Per-round bodies: each round's own text + that round's own tools (design doc §6). */
const roundBodies = computed(() =>
  buildSubAgentRoundsFromScoped(
    scopedTraceMessages.value,
    effectiveAnchorId.value,
    props.trace.id,
    props.trace.agentInstanceId
  )
)

function visibleRoundTools(calls: readonly ToolCall[]): ToolCall[] {
  if (!props.messageUi.showToolCalls) return []
  // Scoped copies can be thinner than the session copy (see innerToolCalls) — keep
  // the round's membership but render the richest copy of each id.
  const byId = new Map(innerToolCalls.value.map(tc => [tc.id, tc]))
  return visibleToolCalls(
    calls
      .map(tc => byId.get(tc.id) ?? tc)
      .filter(tc => !isSubResponseToolName(tc.name)),
    props.messageUi.hideToolNames,
    props.messageUi.showSidecarToolCalls === true,
    props.messageUi.showNonSidecarToolCalls !== false
  )
}

const processRounds = computed(() =>
  roundBodies.value.map(round => ({
    key: round.messageId,
    messageId: round.messageId,
    content: round.content,
    streaming: round.contentStreaming,
    tools: visibleRoundTools(round.toolCalls)
  }))
)

/** Rounds to paint: this spawn's rounds, else the legacy session's single body. */
const roundsForRender = computed(() => {
  const rounds = processRounds.value
  if (rounds.length > 0) {
    const live = liveInnerTool.value
    if (!live || rounds.some(round => round.tools.some(tc => tc.id === live.id))) return rounds
    return [...rounds, { key: `live-${live.id}`, messageId: '', content: '', streaming: false, tools: [live] }]
  }
  const legacy = legacySession.value
  if (!legacy) return rounds
  return [
    {
      key: 'legacy',
      messageId: '',
      content: '',
      streaming: false,
      tools: visibleRoundTools(legacy.toolCalls ?? [])
    }
  ]
})

/** Collapsed one-line preview of the latest round text (D-E1). */
const contentPreview = computed(() =>
  subAgentContentPreviewLine(latestSubAgentContent(roundBodies.value))
)

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

const summaryLine = computed(() =>
  resolveSubAgentSummaryDisplay({
    statsSummary: collapsedView.value.summaryLine,
    running: isRunning.value,
    liveLine: collapsedView.value.liveLine
  })
)
/** Expanded lists already paint in-flight tools; keep only the thinking gap. */
const liveLine = computed(() => {
  const line = collapsedView.value.liveLine
  if (!line?.trim()) return null
  if (!collapsed.value && liveInnerTool.value) return null
  return line
})
const liveToolName = computed(() => {
  if (!collapsed.value) return null
  return liveInnerTool.value?.name ?? null
})
const keepThinkingLiveWhenExpanded = computed(
  () => !collapsed.value && isRunning.value && !!liveLine.value?.trim() && !liveInnerTool.value
)
const liveAriaLabel = computed(() => {
  const summary = summaryLine.value.trim()
  if (summary) return summary
  if (liveInnerTool.value) {
    return compactToolCallStatusLine(liveInnerTool.value, chatStore.current?.workspaceRoot, {
      includeStatus: false
    })
  }
  return liveLine.value || t('chat.s_fc6dde')
})
const liveKey = computed(() => {
  if (!isRunning.value) return null
  if (collapsed.value && liveInnerTool.value?.id) return liveInnerTool.value.id
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

watch(showRawWire, open => {
  if (!open) return
  for (const msg of scopedMessages.value) {
    if (msg.asideEvicted) void chatStore.ensureMessageAside(msg.id)
  }
  if (legacySession.value?.asideEvicted && props.anchorMessageId) {
    void chatStore.ensureMessageAside(props.anchorMessageId)
  }
})

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

/**
 * Search hit for this frame — including a hit inside a nested spawn, so the
 * collapsed ancestors expand and mount the path to it (design doc §2.5).
 */
const isSearchHit = computed(() => {
  const toolIds = searchToolCallIds.value
  if (toolIds.length > 0 && visibleInnerTools.value.some(tc => toolIds.includes(tc.id))) {
    return true
  }
  return traceSubtreeContainsSearchTarget({
    trace: props.trace,
    ownRows: scopedTraceMessages.value,
    targets: {
      toolCallIds: toolIds,
      contentMessageIds: searchContentIds.value
    },
    rowsForTrace: trace =>
      chatStore.scopedMessagesForTraceCached(
        trace.anchorMessageId?.trim() || effectiveAnchorId.value,
        trace.id,
        trace.agentInstanceId
      )
  })
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
        :force-live-slot="(collapsed && isRunning) || keepThinkingLiveWhenExpanded"
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
        :title="showRawWire ? t('chat.hideRawContent') : t('chat.showRawContent')"
        @click.stop="showRawWire = !showRawWire"
      >
        <Code class="w-3.5 h-3.5" />
      </button>
    </div>

    <!-- D-E1: collapsed frames keep one line of the latest round text. -->
    <div
      v-if="collapsed && contentPreview"
      class="min-w-0 w-full pl-4 pr-2"
      data-sub-agent-content-preview
    >
      <span class="block truncate text-[12px] leading-5 text-muted/70">{{ contentPreview }}</span>
    </div>

    <div
      v-if="!collapsed"
      class="space-y-0.5"
    >
      <ContextCompressingMarker
        v-if="showCompressionMarker"
        :label="compressionProgressLabel"
      />
      <!-- Per-round interleave: that round's content, then that round's tools. -->
      <template
        v-for="round in roundsForRender"
        :key="round.key"
      >
        <SubAgentContentBlock
          v-if="round.content"
          :content="round.content"
          :message-id="round.messageId || undefined"
          :streaming="round.streaming"
          :is-search-match="!!round.messageId && searchContentIds.includes(round.messageId)"
          :is-active-search-match="!!round.messageId && activeSearchContentId === round.messageId"
        />
        <ToolCallRow
          v-for="tc in round.tools"
          :key="tc.id"
          :tool-call="tc"
          :show-tool-call-results="messageUi.showToolCallResults"
          :is-search-match="searchToolCallIds.includes(tc.id)"
          :is-active-search-match="activeSearchToolCallId === tc.id"
        />
      </template>
      <RawWirePanel
        v-if="showRawWire && hasRawWire"
        :reasoning="rawWireReasoning"
        :raw-content="rawWireContent"
        :tool-raw-args="toolRawArgs"
        @close="showRawWire = false"
      />
      <!-- Nested spawns of this worker, rebuilt from this layer's own scoped rows.
           Each child goes through the host so a terminal collapsed child degrades
           to the same stub as a lead-level trace instead of keeping every round. -->
      <SubAgentFrameHost
        v-for="child in childTraces"
        :key="child.id"
        :trace="child"
        :anchor-message-id="effectiveAnchorId"
        :messages="messages"
        :message-ui="messageUi"
        :created-at="createdAt"
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :generating="generating"
        :is-active-generation-message="isActiveGenerationMessage"
        :show-message-actions="showMessageActions"
        :task-board="null"
        :host-tool="null"
      />
    </div>
    <!-- Collapsed: keep pending approval cards actionable (main-turn parity). -->
    <div
      v-else-if="approvalInnerTools.length > 0"
      class="space-y-0.5"
    >
      <ToolCallRow
        v-for="tc in approvalInnerTools"
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
