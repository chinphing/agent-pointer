<script setup lang="ts">
import { computed } from 'vue'
import type { ChatMessage, ToolCall } from '../../../../types/chat'
import { useChatStore } from '../../../../stores/chat'
import { storeToRefs } from 'pinia'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import ToolCallList from '../../ToolCallList.vue'
import AssistantMessageDebugChrome from './AssistantMessageDebugChrome.vue'
import ThinkingIndicator from './ThinkingIndicator.vue'
import { isMessageStreaming } from '../../../../lib/assistantMessageKind'
import { messageHasVisibleStreamingActivity } from '../../../../lib/thinkingIndicator'

const props = defineProps<{
  message: ChatMessage
  toolCalls: ToolCall[]
  messageUi: ResolvedAgentUi
  compactTop?: boolean
  hideFooter?: boolean
}>()

const chatStore = useChatStore()
const { generating, activeGeneratingMessageId } = storeToRefs(chatStore)

const isActiveGenerationMessage = computed(
  () => props.message.id === activeGeneratingMessageId.value
)

const isStreaming = computed(() => isMessageStreaming(props.message.status))

const isRunInProgress = computed(
  () => isStreaming.value || (generating.value && isActiveGenerationMessage.value)
)

const showThinkingIndicator = computed(
  () => isRunInProgress.value && !messageHasVisibleStreamingActivity(props.message)
)
</script>

<template>
  <div
    class="tool-message-segment"
    :class="{ 'chat-hover-root': !hideFooter }"
  >
    <ThinkingIndicator :active="showThinkingIndicator" />

    <div
      v-if="toolCalls.length"
      class="px-3"
      :class="compactTop ? 'tool-block-shell-compact' : 'tool-block-shell'"
    >
      <ToolCallList
        :tool-calls="toolCalls"
        :show-tool-call-results="messageUi.showToolCallResults"
      />
    </div>

    <AssistantMessageDebugChrome
      v-if="!hideFooter"
      :message="message"
      :show-copy="false"
      :generating="generating"
      :is-active-generation-message="isActiveGenerationMessage"
    />
  </div>
</template>
