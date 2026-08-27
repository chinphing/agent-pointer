<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import type { AgentTrace, ChatMessage, ToolCall } from '../../../../types/chat'
import { useSettingsStore } from '../../../../stores/settings'
import { useChatStore } from '../../../../stores/chat'
import { shouldShowSubAgentTrace, uiForSubAgentFrame } from '../../../../lib/agentUi'
import { useAgentsCatalog, uiForMessageAgent } from '../../../../composables/useAgentUi'
import { isMessageStreaming } from '../../../../lib/assistantMessageKind'
import {
  orphanSubTraces,
  subTracesForMessage,
  subTracesForParentToolCall
} from '../../../../lib/subAgentSession'
import { subTaskIdFromTraceId } from '../../../../lib/subAgentStats'
import AgentMessageBody, { type AgentMessageBodyModel } from './AgentMessageBody.vue'
import SubAgentFrame, { type SubAgentTaskBoardBinding } from './SubAgentFrame.vue'
import ModelThoughtPanels from './ModelThoughtPanels.vue'

const props = defineProps<{
  message: ChatMessage
  toolOnly?: boolean
  trailingToolGroups?: { id: string; toolCalls: ToolCall[]; message: ChatMessage }[]
  contentOnly?: boolean
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

const orphanTraces = computed(() =>
  orphanSubTraces(subTraces.value, knownToolCallIds.value)
)

function tracesUnderTool(toolCall: ToolCall): AgentTrace[] {
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

const conversationMessages = computed(() => chatStore.current?.messages ?? [])

const childBoardByTraceId = computed(() => {
  void taskBoards.value
  const out = new Map<string, SubAgentTaskBoardBinding>()
  for (const trace of subTraces.value) {
    const binding = chatStore.childBoardBindingForTrace(
      chatStore.currentId,
      trace.id,
      props.message.id
    )
    if (!binding) continue
    const taskId = subTaskIdFromTraceId(trace.id)
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
  content: props.message.content,
  rawContent: props.message.rawContent,
  contentStreaming: props.message.contentStreaming,
  toolCalls: props.message.toolCalls,
  status: props.message.status,
  createdAt: props.message.createdAt,
  errorMessage: props.message.errorMessage
}))

const showSubAgentFrames = computed(() => !props.contentOnly && showSubAgentTrace.value)
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
        <SubAgentFrame
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
    <SubAgentFrame
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
