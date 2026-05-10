<script setup lang="ts">
import type { ChatMessage } from '../../../types/chat'
import UserMessageBubble from './UserMessageBubble.vue'
import AssistantMessage from './AssistantMessage.vue'

defineProps<{ message: ChatMessage }>()
</script>

<template>
  <UserMessageBubble v-if="message.role === 'user'" :message="message" />
  <AssistantMessage v-else-if="message.role === 'assistant'" :message="message" />
  <div
    v-else
    class="text-xs text-slate-500 border border-white/10 rounded-lg px-3 py-2"
  >
    [{{ message.role }}] {{ message.content }}
  </div>
</template>
