<script setup lang="ts">
import { computed } from 'vue'
import { Bot } from 'lucide-vue-next'
import type { ChatMessage, ToolCall } from '../../../types/chat'
import {
  assistantDisplayKind,
  isEphemeralDesktopNoticeMessage,
  isToolOnlyAssistantMessage
} from '../../../lib/assistantMessageKind'
import { iconForAgentAvatar } from '../../../lib/agentIcons'
import { uiForMessageAgent, useAgentsCatalog } from '../../../composables/useAgentUi'
import { useSettingsStore } from '../../../stores/settings'
import AssistantModelMessage from './assistant/AssistantModelMessage.vue'
import AssistantNoticeMessage from './assistant/AssistantNoticeMessage.vue'
import AssistantErrorMessage from './assistant/AssistantErrorMessage.vue'
import AssistantCancelledMessage from './assistant/AssistantCancelledMessage.vue'

const props = defineProps<{
  message: ChatMessage
  compact?: boolean
  trailingToolGroups?: { id: string; toolCalls: ToolCall[]; message: ChatMessage }[]
  contentOnly?: boolean
}>()

const settings = useSettingsStore()
const agents = useAgentsCatalog()
const kind = computed(() => assistantDisplayKind(props.message))
const messageUi = computed(() =>
  uiForMessageAgent(props.message.agentId, props.message.agentName, settings.settings, agents.value)
)

const avatarIcon = computed(() => iconForAgentAvatar(messageUi.value.avatar))
const toolOnly = computed(() => isToolOnlyAssistantMessage(props.message))
const desktopNotice = computed(() => isEphemeralDesktopNoticeMessage(props.message))
const hideAvatar = computed(
  () =>
    toolOnly.value ||
    desktopNotice.value ||
    props.compact === true ||
    kind.value === 'cancelled'
)
const compactShell = computed(() => toolOnly.value || desktopNotice.value)
</script>

<template>
  <div
    class="chat-hover-root relative chat-column"
    :class="compactShell ? 'tool-only-message' : ''"
  >
    <div
      v-if="!hideAvatar"
      class="message-avatar-slot absolute right-full mr-2 top-0 rounded-lg shrink-0 flex items-center justify-center border border-border"
      :class="
        kind === 'injected_notice'
          ? 'w-6 h-6 bg-hover'
          : 'w-8 h-8 bg-hover'
      "
      :title="messageUi.composerLabel"
    >
      <component
        :is="kind === 'injected_notice' ? Bot : avatarIcon"
        :class="kind === 'injected_notice' ? 'w-3 h-3 text-muted' : 'w-4 h-4 text-muted'"
      />
    </div>

    <div class="w-full min-w-0">
      <AssistantNoticeMessage v-if="kind === 'injected_notice'" :message="message" />
      <AssistantCancelledMessage v-else-if="kind === 'cancelled'" :message="message" />
      <AssistantErrorMessage v-else-if="kind === 'error'" :message="message" />
      <AssistantModelMessage
        v-else
        :message="message"
        :tool-only="toolOnly"
        :trailing-tool-groups="trailingToolGroups"
        :content-only="contentOnly"
      />
    </div>
  </div>
</template>
