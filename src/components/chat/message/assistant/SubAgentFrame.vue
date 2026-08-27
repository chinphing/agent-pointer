<script setup lang="ts">
import { computed, inject, ref, watch, type Ref } from 'vue'
import { Code, GitMerge } from 'lucide-vue-next'
import type { AgentTrace, ChatMessage, TaskBoardDocument, ToolCall } from '../../../../types/chat'
import { traceAgentLabel, type ResolvedAgentUi } from '../../../../lib/agentUi'
import { buildCompressionProgressLabel } from '../../../../lib/compressionMessage'
import {
  resolveCollapsedSubAgentView,
  subAgentIdFromTraceId
} from '../../../../lib/subAgentStats'
import {
  buildSubAgentBodyModelsFromScoped,
  buildSubAgentBodyModelsSplitAtCut,
  buildToolRawArgsFromMessages,
  computeSubAgentStatsFromMessages,
  latestSubAgentBodyModelFromScoped,
  scopedAssistantMessagesForTrace,
  scopedMessagesForTrace,
  subAgentFrameOwnsCompression
} from '../../../../lib/subAgentMessages'
import { useChatStore } from '../../../../stores/chat'
import {
  isSubTraceUiCollapsed,
  toggleSubTraceExpanded
} from '../../../../lib/subAgentSession'
import {
  compactToolCallStatusLine,
  effectiveToolDisplaySummary,
  formatToolDurationLabel,
  isToolCallInProgress,
  latestToolCallForCompactStatus
} from '../../../../lib/toolCallDisplay'
import {
  thinkingLabel,
  streamedCharCountFromBody,
  subAgentThinkingActive
} from '../../../../lib/thinkingIndicator'
import { useSettingsStore } from '../../../../stores/settings'
import { useAgentsCatalog } from '../../../../composables/useAgentUi'
import AgentMessageBody, { type AgentMessageBodyModel } from './AgentMessageBody.vue'
import ContextCompressingMarker from '../ContextCompressingMarker.vue'
import RawWirePanel from './RawWirePanel.vue'
import TaskBoardPanel from '../../TaskBoardPanel.vue'
import CollapsedRunHeader from '../../CollapsedRunHeader.vue'

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
  /** Parent `run_subagent` row this frame replaces when collapsed. */
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

const liveInnerTool = computed(() => {
  if (!isRunning.value) return null
  const latest = latestToolCallForCompactStatus(innerToolCalls.value)
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

const hasFinishedInnerWork = computed(() =>
  innerToolCalls.value.some(tc => !isToolCallInProgress(tc.status))
)

const collapsedView = computed(() => {
  const stats =
    scopedTraceMessages.value.length > 0
      ? computeSubAgentStatsFromMessages(scopedTraceMessages.value)
      : (legacySession.value?.stats ?? { searchCount: 0, readCount: 0 })
  const thinking = showThinkingInSummary.value
    ? thinkingLabel(latestStreamBody.value ? streamedCharCountFromBody(latestStreamBody.value) : 0)
    : null
  const live = liveInnerTool.value
    ? compactToolCallStatusLine(liveInnerTool.value, chatStore.current?.workspaceRoot, {
        includeStatus: false
      })
    : null
  return resolveCollapsedSubAgentView({
    goal: goalLabel.value,
    fallbackLabel: traceLabel.value,
    status: props.trace.status,
    stats,
    agentId: subAgentIdFromTraceId(props.trace.id),
    liveToolLine: live,
    hasFinishedWork: hasFinishedInnerWork.value,
    thinkingLine: thinking
  })
})

const summaryLine = computed(() => collapsedView.value.summaryLine)
const liveLine = computed(() => (collapsed.value ? collapsedView.value.liveLine : null))
const liveKey = computed(() => {
  if (!collapsed.value || !isRunning.value) return null
  if (liveInnerTool.value?.id) return liveInnerTool.value.id
  if (liveLine.value?.trim()) return 'thinking'
  return null
})

const durationLabel = computed(() => {
  if (isRunning.value) return ''
  return formatToolDurationLabel(props.hostTool?.durationMs)
})

const processBodies = computed((): {
  before: AgentMessageBodyModel[]
  after: AgentMessageBodyModel[]
  showMarker: boolean
} => {
  const showMarker = ownsCompression.value && !!compressionProgressLabel.value
  if (showMarker) {
    const split = buildSubAgentBodyModelsSplitAtCut(
      props.messages,
      effectiveAnchorId.value,
      props.trace.id,
      chatStore.contextCompressing?.insertBeforeMessageId,
      props.trace.status,
      props.trace.agentInstanceId
    )
    if (split.before.length > 0 || split.after.length > 0) {
      return { before: split.before, after: split.after, showMarker: true }
    }
  }
  const scoped = buildSubAgentBodyModelsFromScoped(
    props.messages,
    effectiveAnchorId.value,
    props.trace.id,
    props.trace.status,
    props.trace.agentInstanceId
  )
  if (scoped.length > 0) {
    return { before: scoped, after: [], showMarker }
  }
  const s = legacySession.value
  if (!s) return { before: [], after: [], showMarker }
  return {
    before: [
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
    ],
    after: [],
    showMarker
  }
})

const bodyModels = computed((): AgentMessageBodyModel[] => [
  ...processBodies.value.before,
  ...processBodies.value.after
])

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

const isSearchHit = computed(() => {
  const ids = searchToolCallIds.value
  if (ids.length === 0) return false
  const hostId = props.hostTool?.id?.trim()
  if (hostId && ids.includes(hostId)) return true
  return innerToolCalls.value.some(tc => ids.includes(tc.id))
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
    :class="collapsed && !taskBoard ? '' : 'space-y-2'"
    :data-tool-call-id="hostTool?.id || undefined"
    :style="{
      marginLeft: `${Math.max(0, (trace.depth ?? 1) - 1) * 12}px`
    }"
  >
    <!-- Always above process UI (collapsed summary or expanded tool cards). -->
    <div
      v-if="taskBoard"
      class="flex justify-start"
    >
      <TaskBoardPanel
        :document="taskBoard.document"
        :is-active="taskBoard.isActive"
        :conversation-id="taskBoard.conversationId"
        :task-id="taskBoard.taskId"
      />
    </div>

    <div class="flex items-start gap-2 min-w-0">
      <CollapsedRunHeader
        :summary-line="summaryLine"
        :live-line="liveLine"
        :live-key="liveKey"
        :expanded="!collapsed"
        :duration-label="durationLabel"
        :failed="trace.status === 'failed'"
        :force-live-slot="collapsed && isRunning"
        :live-busy="collapsed && isRunning && (!!liveInnerTool || !!liveLine?.trim())"
        indent-live
        :aria-label="`子任务 ${summaryLine}`"
        @toggle="toggleExpanded"
      >
        <template #icon>
          <GitMerge
            class="h-3.5 w-3.5 shrink-0 text-muted/70"
            aria-hidden="true"
          />
        </template>
      </CollapsedRunHeader>
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
      class="space-y-2"
    >
      <AgentMessageBody
        v-for="(body, index) in processBodies.before"
        :key="`${trace.id}-before-${index}`"
        :body="body"
        :message-ui="messageUi"
        hide-response
        hide-copy
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :generating="generating"
        :is-active-generation-message="subFrameActive && processBodies.after.length === 0 && index === activeBodyIndex"
      />
      <ContextCompressingMarker
        v-if="processBodies.showMarker"
        :label="compressionProgressLabel"
      />
      <AgentMessageBody
        v-for="(body, index) in processBodies.after"
        :key="`${trace.id}-after-${index}`"
        :body="body"
        :message-ui="messageUi"
        hide-response
        hide-copy
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :generating="generating"
        :is-active-generation-message="subFrameActive && (processBodies.before.length + index) === activeBodyIndex"
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
