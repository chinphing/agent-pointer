<script setup lang="ts">
import type { ChatMessage } from '../../../types/chat'
import { isCompressionSummaryMessage } from '../../../lib/compressionMessage'
import { isTaskBoardTrimMessage } from '../../../lib/taskBoardTrimMessage'
import UserMessageBubble from './UserMessageBubble.vue'
import CompressionSummaryBubble from './CompressionSummaryBubble.vue'
import TaskBoardTrimBubble from './TaskBoardTrimBubble.vue'
import AssistantMessage from './AssistantMessage.vue'

defineProps<{ message: ChatMessage }>()
</script>

<template>
  <CompressionSummaryBubble v-if="isCompressionSummaryMessage(message)" :message="message" />
  <TaskBoardTrimBubble v-else-if="isTaskBoardTrimMessage(message)" :message="message" />
  <UserMessageBubble v-else-if="message.role === 'user'" :message="message" />
  <AssistantMessage v-else-if="message.role === 'assistant'" :message="message" />
  <div
    v-else
    class="text-xs text-slate-500 border border-white/10 rounded-lg px-3 py-2"
  >
    [{{ message.role }}] {{ message.content }}
  </div>
</template>
