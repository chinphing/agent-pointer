<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import type { ChatMessage, ToolCall } from '../../../../types/chat'
import { useSettingsStore } from '../../../../stores/settings'
import { useChatStore } from '../../../../stores/chat'
import { shouldShowSubAgentTrace, uiForSubAgentFrame } from '../../../../lib/agentUi'
import { useAgentsCatalog, uiForMessageAgent } from '../../../../composables/useAgentUi'
import { isMessageStreaming } from '../../../../lib/assistantMessageKind'
import { subTracesForMessage } from '../../../../lib/subAgentSession'
import AgentMessageBody, { type AgentMessageBodyModel } from './AgentMessageBody.vue'
import SubAgentFrame from './SubAgentFrame.vue'
import ModelThoughtPanels from './ModelThoughtPanels.vue'

const props = defineProps<{
  message: ChatMessage
  toolOnly?: boolean
  trailingToolGroups?: { id: string; toolCalls: ToolCall[]; message: ChatMessage }[]
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

function subTraceUi(trace: (typeof subTraces.value)[number]) {
  return uiForSubAgentFrame(
    trace,
    settingsStore.settings,
    agentsCatalog.value,
    messageUi.value
  )
}

const thoughtsDebugEnabled = computed(() => false)

const chatStore = useChatStore()
const { generating, activeGeneratingMessageId } = storeToRefs(chatStore)

function childTaskBoardForTrace(traceId: string) {
  const convId = chatStore.currentId
  if (!convId) return null
  const taskId = (() => {
    const i = traceId.indexOf(':')
    return i > 0 ? traceId.slice(0, i).trim() : traceId.trim()
  })()
  if (!taskId) return null
  const parentStoreKey =
    chatStore.taskBoardForConversation(convId)?.activeParentStoreKey ?? convId
  return chatStore.childBoardsForParent(convId, parentStoreKey)?.[taskId] ?? null
}

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

const showSupervisorPlan = computed(
  () =>
    showSubAgentTrace.value &&
    (props.message.supervisorPlanTasks?.length ?? 0) > 0 &&
    subTraces.value.length === 0
)
</script>

<template>
  <div
    class="w-full max-w-full"
    :class="toolOnly ? 'space-y-0' : 'space-y-2'"
  >
    <ModelThoughtPanels
      v-if="showSupervisorPlan"
      :plan-tasks="message.supervisorPlanTasks"
      :is-streaming="isStreaming"
    />

    <AgentMessageBody
      :body="leadBody"
      :lead-message="message"
      :message-ui="messageUi"
      :thoughts-debug-enabled="thoughtsDebugEnabled"
      :generating="generating"
      :is-active-generation-message="isActiveGenerationMessage"
      :tool-only="toolOnly"
      :trailing-tool-groups="trailingToolGroups"
    />

    <SubAgentFrame
      v-for="trace in subTraces"
      v-show="showSubAgentTrace"
      :key="trace.id"
      :trace="trace"
      :message-ui="subTraceUi(trace)"
      :created-at="message.createdAt"
      :thoughts-debug-enabled="thoughtsDebugEnabled"
      :generating="generating"
      :is-active-generation-message="isActiveGenerationMessage"
      :show-message-actions="showMessageActions"
      :child-task-board-document="childTaskBoardForTrace(trace.id)"
    />

  </div>
</template>
