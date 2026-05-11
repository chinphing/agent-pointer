<script setup lang="ts">
import { computed } from 'vue'
import { Bot } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import { assistantDisplayKind } from '../../../lib/assistantMessageKind'
import AssistantModelMessage from './assistant/AssistantModelMessage.vue'
import AssistantNoticeMessage from './assistant/AssistantNoticeMessage.vue'
import AssistantErrorMessage from './assistant/AssistantErrorMessage.vue'

const props = defineProps<{ message: ChatMessage }>()

const kind = computed(() => assistantDisplayKind(props.message))
</script>

<template>
  <div class="flex gap-3 flex-row" :class="kind === 'injected_notice' ? 'gap-2' : ''">
    <div
      class="rounded-lg shrink-0 flex items-center justify-center"
      :class="
        kind === 'injected_notice'
          ? 'w-6 h-6 bg-white/5 border border-white/10'
          : 'w-8 h-8 bg-gradient-to-br from-primary via-primary-fuchsia to-primary-cyan shadow-lg shadow-primary/30'
      "
    >
      <Bot :class="kind === 'injected_notice' ? 'w-3 h-3 text-slate-500' : 'w-4 h-4 text-white'" />
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
