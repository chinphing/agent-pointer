<script setup lang="ts">
import { computed } from 'vue'
import { marked } from 'marked'
import { Bot, User, Copy, RotateCcw, AlertCircle } from 'lucide-vue-next'
import type { ChatMessage } from '../../types/chat'
import ToolCallCard from './ToolCallCard.vue'
import { useChatStore } from '../../stores/chat'

const props = defineProps<{ message: ChatMessage }>()
const chat = useChatStore()

marked.setOptions({ breaks: true, gfm: true })
const html = computed(() =>
  props.message.content ? marked.parse(props.message.content) as string : ''
)

const isUser = computed(() => props.message.role === 'user')
const isStreaming = computed(() => props.message.status === 'streaming')

function copy() {
  navigator.clipboard.writeText(props.message.content).catch(e => console.error(e))
}
</script>

<template>
  <div class="flex gap-3" :class="isUser ? 'flex-row-reverse' : 'flex-row'">
    <div
      class="w-8 h-8 rounded-lg shrink-0 flex items-center justify-center"
      :class="isUser
        ? 'bg-gradient-to-br from-slate-600 to-slate-700'
        : 'bg-gradient-to-br from-primary via-primary-fuchsia to-primary-cyan shadow-lg shadow-primary/30'"
    >
      <User v-if="isUser" class="w-4 h-4 text-white" />
      <Bot v-else class="w-4 h-4 text-white" />
    </div>

    <div class="flex-1 min-w-0" :class="isUser ? 'flex flex-col items-end' : ''">
      <div
        class="inline-block max-w-full px-4 py-3 rounded-2xl border"
        :class="isUser
          ? 'bg-primary/15 border-primary/25 text-slate-100'
          : 'glass border-white/5'"
      >
        <div v-if="message.agentTrace && message.agentTrace.length" class="mb-2 rounded-xl border border-primary/20 bg-primary/10 px-3 py-2 text-[11px] text-slate-300">
          <div class="mb-1 font-medium text-primary-cyan">Multi-Agent Trace</div>
          <div v-for="agent in message.agentTrace" :key="agent.id" class="mb-2 last:mb-0">
            <div class="flex gap-2">
              <span class="text-slate-100">{{ agent.name }}</span>
              <span class="text-slate-500">{{ agent.status }}</span>
              <span v-if="agent.detail" class="text-slate-400">{{ agent.detail }}</span>
            </div>
            <pre v-if="agent.content" class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-black/20 p-2 text-[11px] leading-relaxed text-slate-300">{{ agent.content }}</pre>
          </div>
        </div>

        <div v-if="message.reasoning" class="mb-2 text-[12px] text-slate-400 italic border-l-2 border-primary/40 pl-3">
          {{ message.reasoning }}
        </div>

        <div v-if="message.content" v-html="html" class="md-body" />
        <div v-else-if="isStreaming" class="flex items-center text-slate-400 text-sm">
          <span class="typing-dot" />
          <span class="typing-dot" style="animation-delay:.2s" />
          <span class="typing-dot" style="animation-delay:.4s" />
        </div>

        <div v-if="message.status === 'error'" class="mt-2 flex items-center gap-2 text-xs text-danger">
          <AlertCircle class="w-3.5 h-3.5" />
          {{ message.errorMessage || '生成失败' }}
        </div>
      </div>

      <div v-if="message.toolCalls && message.toolCalls.length" class="mt-2 space-y-2 w-full">
        <ToolCallCard v-for="tc in message.toolCalls" :key="tc.id" :tool-call="tc" />
      </div>

      <div v-if="!isUser && message.status === 'done'" class="mt-1.5 flex items-center gap-1">
        <button class="p-1.5 rounded hover:bg-white/5 cursor-pointer text-slate-400 hover:text-slate-200 transition" @click="copy" title="复制">
          <Copy class="w-3.5 h-3.5" />
        </button>
        <button class="p-1.5 rounded hover:bg-white/5 cursor-pointer text-slate-400 hover:text-slate-200 transition" @click="chat.retry()" title="重试">
          <RotateCcw class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>
  </div>
</template>
