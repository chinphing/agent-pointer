<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue'
import Composer from './Composer.vue'
import { useChatStore } from '../../stores/chat'
import { Sparkles } from 'lucide-vue-next'

/** Lazy: pulls in marked + MessageBubble + ToolCallCard; empty state skips this. */
const MessageList = defineAsyncComponent(() => import('./MessageList.vue'))

const chat = useChatStore()
const empty = computed(() => !chat.current || chat.current.messages.length === 0)
</script>

<template>
  <div class="flex-1 flex flex-col min-h-0">
    <div class="flex-1 overflow-hidden relative">
      <div v-if="empty" class="h-full flex flex-col items-center justify-center px-8 text-center">
        <div class="w-14 h-14 rounded-2xl bg-gradient-to-br from-primary via-primary-fuchsia to-primary-cyan flex items-center justify-center shadow-xl shadow-primary/30 mb-4">
          <Sparkles class="w-7 h-7 text-white" />
        </div>
        <h1 class="text-2xl font-bold gradient-text mb-1.5">你好，欢迎来到 Pointer</h1>
        <p class="text-slate-400 max-w-md text-sm leading-6">
          你的 AI 智能助手，可以回答问题、写作、分析数据、执行任务。
        </p>
      </div>

      <MessageList v-else />
    </div>
    <Composer />
  </div>
</template>
