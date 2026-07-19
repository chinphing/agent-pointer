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
import { isTaskBoardTerminal } from '../../../../stores/chat/taskBoard'
import AgentMessageBody, { type AgentMessageBodyModel } from './AgentMessageBody.vue'
import SubAgentFrame from './SubAgentFrame.vue'
import ModelThoughtPanels from './ModelThoughtPanels.vue'
import TaskBoardPanel from '../../TaskBoardPanel.vue'

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
  const out = new Map<
    string,
    { storeKey: string; document: import('../../../../types/chat').TaskBoardDocument; isActive: boolean }
  >()
  for (const trace of subTraces.value) {
    const binding = chatStore.childBoardBindingForTrace(
      chatStore.currentId,
      trace.id,
      props.message.id
    )
    if (binding) out.set(trace.id, binding)
  }
  return out
})

function childBoardStickyClass(trace: AgentTrace): string {
  const doc = childBoardByTraceId.value.get(trace.id)?.document
  return isTaskBoardTerminal(doc?.meta?.status)
    ? ''
    : 'sticky top-0 z-20 bg-background/95 backdrop-blur-sm'
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
    >
      <template #after-tool="{ toolCall }">
        <template
          v-for="trace in tracesUnderTool(toolCall)"
          :key="trace.id"
        >
          <div
            v-if="childBoardByTraceId.get(trace.id)"
            v-show="showSubAgentTrace"
            class="task-board-sticky mb-1 mt-1 flex justify-start py-1"
            :class="childBoardStickyClass(trace)"
          >
            <TaskBoardPanel
              :document="childBoardByTraceId.get(trace.id)!.document"
              :is-active="childBoardByTraceId.get(trace.id)!.isActive"
              :conversation-id="chatStore.currentId"
              :task-id="subTaskIdFromTraceId(trace.id)"
            />
          </div>

          <SubAgentFrame
            v-show="showSubAgentTrace"
            class="mt-1 mb-1"
            :trace="trace"
            :anchor-message-id="message.id"
            :messages="conversationMessages"
            :message-ui="subTraceUi(trace)"
            :created-at="message.createdAt"
            :thoughts-debug-enabled="thoughtsDebugEnabled"
            :generating="generating"
            :is-active-generation-message="isActiveGenerationMessage"
            :show-message-actions="showMessageActions"
          />
        </template>
      </template>
    </AgentMessageBody>

    <!-- Legacy / unmatched traces (no parentToolCallId or tool row missing). -->
    <template
      v-for="trace in orphanTraces"
      :key="trace.id"
    >
      <div
        v-if="childBoardByTraceId.get(trace.id)"
        v-show="showSubAgentTrace"
        class="task-board-sticky mb-1 flex justify-start py-1"
        :class="childBoardStickyClass(trace)"
      >
        <TaskBoardPanel
          :document="childBoardByTraceId.get(trace.id)!.document"
          :is-active="childBoardByTraceId.get(trace.id)!.isActive"
          :conversation-id="chatStore.currentId"
          :task-id="subTaskIdFromTraceId(trace.id)"
        />
      </div>

      <SubAgentFrame
        v-show="showSubAgentTrace"
        :trace="trace"
        :anchor-message-id="message.id"
        :messages="conversationMessages"
        :message-ui="subTraceUi(trace)"
        :created-at="message.createdAt"
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :generating="generating"
        :is-active-generation-message="isActiveGenerationMessage"
        :show-message-actions="showMessageActions"
      />
    </template>
  </div>
</template>
