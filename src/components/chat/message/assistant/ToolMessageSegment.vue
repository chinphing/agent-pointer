<script setup lang="ts">
import { computed } from 'vue'
import type { ChatMessage, ToolCall } from '../../../../types/chat'
import { useChatStore } from '../../../../stores/chat'
import { storeToRefs } from 'pinia'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import { useSettingsStore } from '../../../../stores/settings'
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
  /** Compact tool runs hide footer by default; debug UI settings override unless delegated. */
  hideFooter?: boolean
  /** Parent renders AssistantMessageDebugChrome for this message — do not duplicate debug controls. */
  delegatedDebugFooter?: boolean
}>()

const settingsStore = useSettingsStore()

const effectiveHideFooter = computed(() => {
  if (props.delegatedDebugFooter) return true
  const st = settingsStore.settings
  const debugChrome =
    st.computerAnnotatedScreenViewEnabled === true || st.rawContentViewEnabled === true
  if (debugChrome) return false
  return props.hideFooter === true
})

const chatStore = useChatStore()
const { generating, activeGeneratingMessageId } = storeToRefs(chatStore)

const isActiveGenerationMessage = computed(
  () => props.message.id === activeGeneratingMessageId.value
)

const isStreaming = computed(() => isMessageStreaming(props.message.status))

const isRunInProgress = computed(
  () => isStreaming.value || (generating.value && isActiveGenerationMessage.value)
)

const streamedCharCount = computed(() => {
  const m = props.message
  return Math.max(
    m.content?.length ?? 0,
    m.thoughts?.length ?? 0,
    m.reasoning?.length ?? 0,
    m.toolNamePreview?.length ?? 0,
    m.responseTextDraft?.length ?? 0
  )
})

const showThinkingIndicator = computed(
  () => isRunInProgress.value && !messageHasVisibleStreamingActivity(props.message)
)
</script>

<template>
  <div
    class="tool-message-segment"
    :class="{ 'chat-hover-root': !effectiveHideFooter }"
  >
    <ThinkingIndicator :active="showThinkingIndicator" :char-count="streamedCharCount" />

    <div
      v-if="toolCalls.length"
      class="px-3"
      :class="compactTop ? 'tool-block-shell-compact' : 'tool-block-shell'"
    >
      <ToolCallList
        :tool-calls="toolCalls"
        :show-tool-call-results="messageUi.showToolCallResults"
      >
        <template #after-tool="slotProps">
          <slot
            name="after-tool"
            v-bind="slotProps"
          />
        </template>
      </ToolCallList>
    </div>

    <AssistantMessageDebugChrome
      v-if="!effectiveHideFooter"
      :message="message"
      :show-copy="false"
      :generating="generating"
      :is-active-generation-message="isActiveGenerationMessage"
    />
  </div>
</template>
