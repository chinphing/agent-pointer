<script setup lang="ts">
import { computed } from 'vue'
import type { ChatMessage, ToolCall } from '../../../../types/chat'
import { useChatStore } from '../../../../stores/chat'
import { storeToRefs } from 'pinia'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import ToolCallList from '../../ToolCallList.vue'
import {
  collapsedProcessRunActive,
  thinkingLabel,
  thinkingCharCountForCurrentRound
} from '../../../../lib/thinkingIndicator'

const props = defineProps<{
  message: ChatMessage
  toolCalls: ToolCall[]
  messageUi: ResolvedAgentUi
  compactTop?: boolean
}>()

const chatStore = useChatStore()
const { generating, activeGeneratingMessageId } = storeToRefs(chatStore)

const isActiveGenerationMessage = computed(
  () => props.message.id === activeGeneratingMessageId.value
)

const isRunInProgress = computed(() =>
  collapsedProcessRunActive({
    generating: generating.value,
    isActiveHost: isActiveGenerationMessage.value,
    host: props.message,
    toolCalls: props.toolCalls
  })
)

const streamedCharCount = computed(() => thinkingCharCountForCurrentRound(props.message))

const thinkingLine = computed(() => {
  if (!isRunInProgress.value) return null
  return thinkingLabel(streamedCharCount.value)
})

const showToolSlot = computed(
  () => props.toolCalls.length > 0 || !!thinkingLine.value
)
</script>

<template>
  <div class="tool-message-segment">
    <div
      v-if="showToolSlot"
      class="min-w-0 max-w-full px-3"
      :class="compactTop ? 'tool-block-shell-compact' : 'tool-block-shell'"
    >
      <ToolCallList
        :tool-calls="toolCalls"
        :show-tool-call-results="messageUi.showToolCallResults"
        :run-active="isRunInProgress"
        :thinking-line="thinkingLine"
      >
        <template #after-tool="slotProps">
          <slot
            name="after-tool"
            v-bind="slotProps"
          />
        </template>
      </ToolCallList>
    </div>
  </div>
</template>
