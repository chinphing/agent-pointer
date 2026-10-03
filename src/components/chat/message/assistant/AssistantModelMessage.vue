<script setup lang="ts">
import { computed, onMounted, onUpdated, watch } from 'vue'
import { bump, renderPerfEnabled } from '../../../../lib/renderPerf'

// Dev-only render perf counters — one boolean check, no-op unless the HUD is on.
onMounted(() => {
  if (!renderPerfEnabled()) return
  bump('mount:AssistantModelMessage')
})
onUpdated(() => {
  if (!renderPerfEnabled()) return
  bump('render:AssistantModelMessage')
})
import { storeToRefs } from 'pinia'
import type { AgentTrace, ChatMessage, ToolCall } from '../../../../types/chat'
import { useSettingsStore } from '../../../../stores/settings'
import { useChatStore } from '../../../../stores/chat'
import { shouldShowSubAgentTrace, uiForSubAgentFrame } from '../../../../lib/agentUi'
import { hostNeedsCollapsedSubAgentFrames } from '../../../../lib/messageTooling'
import { useAgentsCatalog, uiForMessageAgent } from '../../../../composables/useAgentUi'
import { isMessageStreaming } from '../../../../lib/assistantMessageKind'
import { stripOutboundMediaMarkers } from '../../../../lib/outboundMedia'
import {
  orphanSubTraces,
  subTracesForMessage,
  subTracesForParentToolCall
} from '../../../../lib/subAgentSession'
import {
  framesForMessage,
  missingHostFetches,
  type TurnSpawnPlan
} from '../../../../lib/subAgentRenderTree'
import { resolveTraceTaskId, traceLookupId } from '../../../../lib/subAgentStats'
import AgentMessageBody, { type AgentMessageBodyModel } from './AgentMessageBody.vue'
import SubAgentFrameHost, { type SubAgentTaskBoardBinding } from './SubAgentFrameHost.vue'
import ModelThoughtPanels from './ModelThoughtPanels.vue'

const props = defineProps<{
  message: ChatMessage
  toolOnly?: boolean
  trailingToolGroups?: { id: string; toolCalls: ToolCall[]; message: ChatMessage }[]
  contentOnly?: boolean
  /**
   * 回合级归属计划（P1）。有它时：子帧由**数据归属**决定（谁画宿主行，谁画子帧），
   * 与本消息的 `agentTrace` 数组无关；没有它时保持旧的「消息局部配对」行为。
   */
  spawnPlan?: TurnSpawnPlan | null
}>()

const settingsStore = useSettingsStore()
const agentsCatalog = useAgentsCatalog()
const messageUi = computed(() =>
  uiForMessageAgent(
    props.message.agentId,
    props.message.agentName,
    settingsStore.settings,
    agentsCatalog.value
  )
)

const showSubAgentTrace = computed(() =>
  shouldShowSubAgentTrace(
    messageUi.value,
    props.message.agentTrace,
    settingsStore.settings,
    props.message.agentId
  )
)

const subTraces = computed(() => subTracesForMessage(props.message))

/**
 * P1：回合级归属计划。有注入时，子帧由**数据归属**决定（谁画宿主行，谁画子帧），
 * 与本消息的 `agentTrace` 数组无关；没有注入时保持旧的「消息局部配对」行为。
 */
const spawnFrames = computed(() =>
  props.spawnPlan ? framesForMessage(props.spawnPlan, props.message) : null
)

const knownToolCallIds = computed(() => {
  const ids = new Set<string>()
  for (const tc of props.message.toolCalls ?? []) {
    if (tc.id?.trim()) ids.add(tc.id.trim())
  }
  for (const group of props.trailingToolGroups ?? []) {
    for (const tc of group.toolCalls) {
      if (tc.id?.trim()) ids.add(tc.id.trim())
    }
  }
  return ids
})

/**
 * 兜底：只放**本回合任何地方都没画出来**的档案（宿主行被折叠隐藏，或本回合根本没有宿主行）。
 * 宿主行已在别处画出来的档案不会进这里 —— 避免重复渲染。
 */
const orphanTraces = computed(() => {
  const planned = spawnFrames.value
  if (planned) return [...planned.unplaced, ...planned.unbound]
  return orphanSubTraces(subTraces.value, knownToolCallIds.value)
})

function tracesUnderTool(toolCall: ToolCall): AgentTrace[] {
  const planned = spawnFrames.value
  if (planned) return planned.byRow.get((toolCall.id ?? '').trim()) ?? []
  return subTracesForParentToolCall(subTraces.value, toolCall.id)
}

function subTraceUi(trace: AgentTrace) {
  return uiForSubAgentFrame(
    trace,
    settingsStore.settings,
    agentsCatalog.value,
    messageUi.value
  )
}

const thoughtsDebugEnabled = computed(
  () => settingsStore.settings.debugMenusEnabled === true
)

const chatStore = useChatStore()
const { generating, activeGeneratingMessageId, taskBoards } = storeToRefs(chatStore)

/**
 * P0.1：宿主行还没加载时，按锚**精确补拉一次**。
 * 嵌套层（第 2 层及更深）的派发行写在父 worker 的 scoped 行上，那条行未加载 / 被驱逐时
 * 回合索引里没有它 → 会被误判成「未关联」。这里只补拉一次，失败就停在未关联组，不重试。
 */
const requestedHostFetches = new Set<string>()
watch(
  () =>
    props.spawnPlan
      ? missingHostFetches(props.spawnPlan, props.message, requestedHostFetches)
      : [],
  items => {
    for (const item of items) {
      requestedHostFetches.add(item.traceId)
      void chatStore.ensureScopedMessagesForTrace(
        item.anchorMessageId,
        item.traceId,
        item.agentInstanceId
      )
    }
  },
  { immediate: true }
)

const conversationMessages = computed(() => chatStore.current?.messages ?? [])

const childBoardByTraceId = computed(() => {
  void taskBoards.value
  const out = new Map<string, SubAgentTaskBoardBinding>()
  for (const trace of subTraces.value) {
    const binding = chatStore.childBoardBindingForTrace(
      chatStore.currentId,
      traceLookupId(trace),
      props.message.id
    )
    if (!binding) continue
    const taskId = resolveTraceTaskId(trace)
    if (!taskId) continue
    out.set(trace.id, {
      document: binding.document,
      isActive: binding.isActive,
      conversationId: chatStore.currentId,
      taskId
    })
  }
  return out
})

const isActiveGenerationMessage = computed(
  () => props.message.id === activeGeneratingMessageId.value
)

watch(
  () => [props.message.asideEvicted, thoughtsDebugEnabled.value, messageUi.value.showReasoning] as const,
  ([evicted, debugThoughts, showReasoning]) => {
    if (!evicted) return
    if (!debugThoughts && !showReasoning) return
    void chatStore.ensureMessageAside(props.message.id)
  },
  { immediate: true }
)

const isStreaming = computed(() => isMessageStreaming(props.message.status))

const showMessageActions = computed(() => {
  if (generating.value && isActiveGenerationMessage.value) return false
  if (props.message.status === 'pending') return false
  return true
})

const leadBody = computed((): AgentMessageBodyModel => ({
  thoughts: props.message.thoughts,
  toolNamePreview: props.message.toolNamePreview,
  responseTextDraft: props.message.responseTextDraft,
  reasoning: props.message.reasoning,
  content: stripOutboundMediaMarkers(props.message.content ?? ''),
  rawContent: props.message.rawContent,
  contentStreaming: props.message.contentStreaming,
  toolCalls: props.message.toolCalls,
  status: props.message.status,
  createdAt: props.message.createdAt,
  errorMessage: props.message.errorMessage
}))

const showSubAgentFrames = computed(() => {
  // P1：本消息画出来的行上有子帧 → 必须画（即使本消息自身没携带 agentTrace）。
  if ((spawnFrames.value?.byRow.size ?? 0) > 0) return true
  if (!showSubAgentTrace.value) return false
  if (!props.contentOnly) return true
  // Collapsed turns used to hide the whole SubAgentFrame (contentOnly), which
  // buried nested ask_user from coder/self. Keep frames that still need a surface.
  return hostNeedsCollapsedSubAgentFrames(props.message, props.trailingToolGroups)
})
</script>

<template>
  <div
    class="w-full max-w-full"
    :class="toolOnly ? 'space-y-0' : 'space-y-2'"
  >
    <AgentMessageBody
      :body="leadBody"
      :lead-message="message"
      :message-ui="messageUi"
      :thoughts-debug-enabled="thoughtsDebugEnabled"
      :generating="generating"
      :is-active-generation-message="isActiveGenerationMessage"
      :tool-only="toolOnly"
      :trailing-tool-groups="trailingToolGroups"
      :content-only="contentOnly"
    >
      <template v-if="showSubAgentFrames" #after-tool="{ toolCall }">
        <SubAgentFrameHost
          v-for="trace in tracesUnderTool(toolCall)"
          v-show="showSubAgentTrace"
          :key="trace.id"
          :trace="trace"
          :anchor-message-id="message.id"
          :messages="conversationMessages"
          :message-ui="subTraceUi(trace)"
          :created-at="message.createdAt"
          :thoughts-debug-enabled="thoughtsDebugEnabled"
          :generating="generating"
          :is-active-generation-message="isActiveGenerationMessage"
          :show-message-actions="showMessageActions"
          :task-board="childBoardByTraceId.get(trace.id) ?? null"
          :host-tool="toolCall"
        />
      </template>
    </AgentMessageBody>

    <!-- Legacy / unmatched traces (no parentToolCallId or tool row missing). -->
    <SubAgentFrameHost
      v-for="trace in orphanTraces"
      v-show="showSubAgentFrames"
      :key="trace.id"
      class="px-3"
      :trace="trace"
      :anchor-message-id="message.id"
      :messages="conversationMessages"
      :message-ui="subTraceUi(trace)"
      :created-at="message.createdAt"
      :thoughts-debug-enabled="thoughtsDebugEnabled"
      :generating="generating"
      :is-active-generation-message="isActiveGenerationMessage"
      :show-message-actions="showMessageActions"
      :task-board="childBoardByTraceId.get(trace.id) ?? null"
    />
  </div>
</template>
