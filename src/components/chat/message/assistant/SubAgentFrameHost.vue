<script setup lang="ts">
import { computed, inject, ref, toRef, watch, type Ref } from 'vue'
import type { AgentTrace, ChatMessage, ToolCall } from '../../../../types/chat'
import { useChatStore } from '../../../../stores/chat'
import { useSettingsStore } from '../../../../stores/settings'
import { useAgentsCatalog } from '../../../../composables/useAgentUi'
import { traceAgentLabel } from '../../../../lib/agentUi'
import { useTerminalSubAgentStub } from '../../../../composables/useTerminalSubAgentStub'
import {
  buildTerminalSubAgentStubView,
  persistTerminalTraceSummaryLine,
  rememberTraceSearchToolCallIds,
  shouldRenderTerminalSubAgentStub
} from '../../../../lib/subAgentFrameMount'
import { isPendingApprovalToolCall } from '../../../../lib/messageTooling'
import { isSubAgentTraceTerminal, toggleSubTraceExpanded } from '../../../../lib/subAgentSession'
import { SUB_AGENT_PROCESS_PLACEHOLDER } from '../../../../lib/subAgentStats'
import SubAgentFrame, { type SubAgentTaskBoardBinding } from './SubAgentFrame.vue'
import SubAgentFrameStub from './SubAgentFrameStub.vue'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'

export type { SubAgentTaskBoardBinding }

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
  taskBoard?: SubAgentTaskBoardBinding | null
  hostTool?: ToolCall | null
}>()

const chatStore = useChatStore()
const settingsStore = useSettingsStore()
const agentsCatalog = useAgentsCatalog()
const traceRef = toRef(props, 'trace')

const traceLabel = computed(() =>
  traceAgentLabel(props.trace, agentsCatalog.value, settingsStore.settings)
)

const searchToolCallIds = inject<Ref<string[]>>(
  'currentConversationSearchToolCallIds',
  ref<string[]>([])
)

const effectiveAnchorId = computed(
  () => props.trace.anchorMessageId?.trim() || props.anchorMessageId
)

const scopedForStub = computed(() => {
  // Stub with a persisted line does not need spawn rows (and they are often evicted).
  if (
    isSubAgentTraceTerminal(props.trace.status)
    && props.trace.userExpanded !== true
    && !!props.trace.summaryLine?.trim()
  ) {
    return [] as ChatMessage[]
  }
  return chatStore.scopedMessagesForTraceCached(
    effectiveAnchorId.value,
    props.trace.id,
    props.trace.agentInstanceId
  )
})

const searchPinned = computed(() => {
  const ids = new Set(
    (searchToolCallIds.value ?? []).map(id => id.trim()).filter(Boolean)
  )
  if (ids.size === 0) return false
  for (const id of props.trace.searchToolCallIds ?? []) {
    if (ids.has(id.trim())) return true
  }
  for (const tc of props.trace.session?.toolCalls ?? []) {
    const id = tc.id?.trim()
    if (id && ids.has(id)) return true
  }
  for (const msg of scopedForStub.value) {
    for (const tc of msg.toolCalls ?? []) {
      const id = tc.id?.trim()
      if (id && ids.has(id)) return true
    }
  }
  return false
})

const { stubIdleElapsed } = useTerminalSubAgentStub(traceRef, {
  pinned: searchPinned
})

/** Pending approval must stay on the full frame; the stub has no approval card. */
const approvalBlocksStub = computed(() => {
  if ((props.trace.session?.toolCalls ?? []).some(isPendingApprovalToolCall)) return true
  if (!isSubAgentTraceTerminal(props.trace.status)) return false
  const rows = chatStore.scopedMessagesForTraceCached(
    effectiveAnchorId.value,
    props.trace.id,
    props.trace.agentInstanceId
  )
  return rows.some(msg => (msg.toolCalls ?? []).some(isPendingApprovalToolCall))
})

const showStub = computed(() =>
  shouldRenderTerminalSubAgentStub(props.trace, stubIdleElapsed.value)
  && !searchPinned.value
  && !approvalBlocksStub.value
)

const stubView = computed(() =>
  buildTerminalSubAgentStubView({
    trace: props.trace,
    hostTool: props.hostTool,
    scopedTraceMessages: scopedForStub.value,
    orphanTitle: props.hostTool ? '' : traceLabel.value
  })
)

const stubSummaryLine = computed(
  () => stubView.value.summaryLine.trim() || SUB_AGENT_PROCESS_PLACEHOLDER
)

function onStubToggle() {
  toggleSubTraceExpanded(props.trace)
}

const frameMemoDeps = computed(() => {
  const board = props.taskBoard
  const boardKey = board
    ? `${board.taskId}:${board.document.version}:${board.isActive ? 1 : 0}`
    : ''
  const expanded = props.trace.userExpanded === true
  const search = searchPinned.value ? 1 : 0
  const compressing = chatStore.contextCompressing
  const compressKey =
    compressing?.scope === 'sub_agent'
      ? `${compressing.insertBeforeMessageId ?? ''}:${compressing.messageId ?? ''}`
      : ''
  const approval = (props.trace.session?.toolCalls ?? [])
    .filter(isPendingApprovalToolCall)
    .map(tc => `${tc.id}:${tc.status}:${tc.arguments?.length ?? 0}:${tc.displaySummary?.length ?? 0}`)
    .join(';')
  if (props.trace.status === 'running') {
    const live = chatStore.getSubAgentLiveSignal(
      props.trace.agentInstanceId?.trim() || props.trace.id
    )
    return [props.trace.status, expanded, boardKey, live, search, compressKey, approval]
  }
  return [
    props.trace.status,
    expanded,
    boardKey,
    isSubAgentTraceTerminal(props.trace.status) ? 1 : 0,
    search,
    approval
  ]
})

watch(
  () => showStub.value,
  stub => {
    if (stub) {
      rememberTraceSearchToolCallIds(
        props.trace,
        chatStore.scopedMessagesForTraceCached(
          effectiveAnchorId.value,
          props.trace.id,
          props.trace.agentInstanceId
        )
      )
      const spawn = props.trace.agentInstanceId?.trim() || props.trace.id
      if (spawn && chatStore.currentId) {
        chatStore.evictScopedInstance(chatStore.currentId, {
          agentInstanceId: props.trace.agentInstanceId,
          anchorMessageId: effectiveAnchorId.value,
          traceId: props.trace.id
        }, 'stub')
      }
      return
    }
    if (!isSubAgentTraceTerminal(props.trace.status)) return
    void chatStore.ensureScopedMessagesForTrace(
      effectiveAnchorId.value,
      props.trace.id,
      props.trace.agentInstanceId
    )
  },
  { immediate: true }
)

const summaryBackfillStarted = ref(false)

watch(
  () => props.trace.summaryLine?.trim() || '',
  () => {
    if (summaryBackfillStarted.value) return
    if (!isSubAgentTraceTerminal(props.trace.status)) return
    if (props.trace.userExpanded === true) return
    if (props.trace.summaryLine?.trim()) return
    summaryBackfillStarted.value = true
    void (async () => {
      await chatStore.ensureScopedMessagesForTrace(
        effectiveAnchorId.value,
        props.trace.id,
        props.trace.agentInstanceId
      )
      persistTerminalTraceSummaryLine(props.trace, {
        hostTool: props.hostTool,
        scopedTraceMessages: chatStore.scopedMessagesForTraceCached(
          effectiveAnchorId.value,
          props.trace.id,
          props.trace.agentInstanceId
        ),
        orphanTitle: props.hostTool ? '' : traceLabel.value
      })
      if (props.trace.userExpanded === true) return
      const spawn = props.trace.agentInstanceId?.trim() || props.trace.id
      if (spawn && chatStore.currentId) {
        chatStore.evictScopedInstance(chatStore.currentId, {
          agentInstanceId: props.trace.agentInstanceId,
          anchorMessageId: effectiveAnchorId.value,
          traceId: props.trace.id
        }, 'stub')
      }
    })()
  },
  { immediate: true }
)
</script>

<template>
  <SubAgentFrameStub
    v-if="showStub"
    :trace="trace"
    :summary-line="stubSummaryLine"
    :show-chevron="true"
    @toggle="onStubToggle"
  />
  <SubAgentFrame
    v-else
    v-memo="frameMemoDeps"
    :trace="trace"
    :anchor-message-id="anchorMessageId"
    :messages="messages"
    :message-ui="messageUi"
    :created-at="createdAt"
    :thoughts-debug-enabled="thoughtsDebugEnabled"
    :generating="generating"
    :is-active-generation-message="isActiveGenerationMessage"
    :show-message-actions="showMessageActions"
    :task-board="taskBoard"
    :host-tool="hostTool"
  />
</template>
