<script setup lang="ts">
import type { ChatMessage } from '../../../types/chat'
import { isCompressionSummaryMessage } from '../../../lib/compressionMessage'
import UserMessageBubble from './UserMessageBubble.vue'
import CompressionSummaryBubble from './CompressionSummaryBubble.vue'
import AssistantMessage from './AssistantMessage.vue'

defineProps<{ message: ChatMessage }>()
</script>

<template>
  <CompressionSummaryBubble v-if="isCompressionSummaryMessage(message)" :message="message" />
  <UserMessageBubble v-else-if="message.role === 'user'" :message="message" />
  <AssistantMessage v-else-if="message.role === 'assistant'" :message="message" />
  <div
    v-else
    class="text-xs text-slate-500 border border-white/10 rounded-lg px-3 py-2"
  >
    [{{ message.role }}] {{ message.content }}
  </div>
</template>
