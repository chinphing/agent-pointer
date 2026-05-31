<script setup lang="ts">
import { computed } from 'vue'
import { Bot } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import { assistantDisplayKind } from '../../../lib/assistantMessageKind'
import { iconForAgentAvatar } from '../../../lib/agentIcons'
import { uiForMessageAgent, useAgentsCatalog } from '../../../composables/useAgentUi'
import { useSettingsStore } from '../../../stores/settings'
import AssistantModelMessage from './assistant/AssistantModelMessage.vue'
import AssistantNoticeMessage from './assistant/AssistantNoticeMessage.vue'
import AssistantErrorMessage from './assistant/AssistantErrorMessage.vue'

const props = defineProps<{ message: ChatMessage }>()

const settings = useSettingsStore()
const agents = useAgentsCatalog()
const kind = computed(() => assistantDisplayKind(props.message))
const messageUi = computed(() =>
  uiForMessageAgent(props.message.agentId, props.message.agentName, settings.settings, agents.value)
)

const avatarIcon = computed(() => iconForAgentAvatar(messageUi.value.avatar))

const hasVisibleBodyText = computed(() => !!(props.message.content?.trim() || props.message.reasoning?.trim()))
const hasToolCards = computed(() => (props.message.toolCalls?.length ?? 0) > 0)
const isToolOnlyAssistantRow = computed(() => hasToolCards.value && !hasVisibleBodyText.value)
</script>

<template>
  <div class="flex gap-3 flex-row" :class="kind === 'injected_notice' ? 'gap-2' : ''">
    <div
      class="rounded-lg shrink-0 flex items-center justify-center border border-border"
      :class="
        kind === 'injected_notice'
          ? 'w-6 h-6 bg-hover'
          : 'w-8 h-8 bg-accent/15'
      "
    >
      <component
        :is="kind === 'injected_notice' ? Bot : avatarIcon"
        :class="kind === 'injected_notice' ? 'w-3 h-3 text-muted' : 'w-4 h-4 text-accent'"
      />
    </div>

    <div
      class="min-w-0 flex flex-col w-full"
      :class="kind === 'injected_notice' ? '' : 'flex-1'"
    >
      <AssistantNoticeMessage v-if="kind === 'injected_notice'" :message="message" />
      <div v-else-if="kind === 'error'" class="w-full min-w-0">
        <AssistantErrorMessage :message="message" />
      </div>
      <AssistantModelMessage v-else class="w-full min-w-0" :message="message" />
    </div>
  </div>
</template>
