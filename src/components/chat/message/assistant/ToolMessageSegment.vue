<script setup lang="ts">
import { computed } from 'vue'
import type { ChatMessage, ToolCall } from '../../../../types/chat'
import { useChatStore } from '../../../../stores/chat'
import { storeToRefs } from 'pinia'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import ToolCallList from '../../ToolCallList.vue'
import AssistantMessageDebugChrome from './AssistantMessageDebugChrome.vue'

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
</script>

<template>
  <div
    class="tool-message-segment"
    :class="{ 'chat-hover-root': !hideFooter }"
  >
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
