<script setup lang="ts">
import { computed } from 'vue'
import { Bot } from 'lucide-vue-next'
import type { ChatMessage, ToolCall } from '../../../types/chat'
import { assistantDisplayKind, isToolOnlyAssistantMessage } from '../../../lib/assistantMessageKind'
import { iconForAgentAvatar } from '../../../lib/agentIcons'
import { uiForMessageAgent, useAgentsCatalog } from '../../../composables/useAgentUi'
import { useSettingsStore } from '../../../stores/settings'
import AssistantModelMessage from './assistant/AssistantModelMessage.vue'
import AssistantNoticeMessage from './assistant/AssistantNoticeMessage.vue'
import AssistantErrorMessage from './assistant/AssistantErrorMessage.vue'

const props = defineProps<{
  message: ChatMessage
  compact?: boolean
  trailingToolGroups?: { id: string; toolCalls: ToolCall[]; message: ChatMessage }[]
}>()

const settings = useSettingsStore()
const agents = useAgentsCatalog()
const kind = computed(() => assistantDisplayKind(props.message))
const messageUi = computed(() =>
  uiForMessageAgent(props.message.agentId, props.message.agentName, settings.settings, agents.value)
)

const avatarIcon = computed(() => iconForAgentAvatar(messageUi.value.avatar))
const toolOnly = computed(() => isToolOnlyAssistantMessage(props.message))
const hideAvatar = computed(() => toolOnly.value || props.compact === true)
</script>

<template>
  <div
    class="chat-hover-root relative chat-column"
    :class="toolOnly ? 'tool-only-message' : ''"
  >
    <div
      v-if="!hideAvatar"
      class="message-avatar-slot absolute right-full mr-2 top-0 rounded-lg shrink-0 flex items-center justify-center border border-border"
      :class="
        kind === 'injected_notice'
          ? 'w-6 h-6 bg-hover'
          : 'w-8 h-8 bg-accent/15'
      "
      :title="messageUi.composerLabel"
    >
      <component
        :is="kind === 'injected_notice' ? Bot : avatarIcon"
        :class="kind === 'injected_notice' ? 'w-3 h-3 text-muted' : 'w-4 h-4 text-accent'"
      />
    </div>

    <div class="w-full min-w-0">
      <AssistantNoticeMessage v-if="kind === 'injected_notice'" :message="message" />
      <AssistantErrorMessage v-else-if="kind === 'error'" :message="message" />
      <AssistantModelMessage
        v-else
        :message="message"
        :tool-only="toolOnly"
        :trailing-tool-groups="trailingToolGroups"
      />
    </div>
  </div>
</template>
