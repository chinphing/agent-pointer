<script setup lang="ts">
import { computed, onMounted, ref, watch, nextTick } from 'vue'
import { marked } from 'marked'
import { Bot, User, Copy, AlertCircle, Check, Code } from 'lucide-vue-next'
import type { ChatMessage, ToolCall } from '../../types/chat'
import ToolCallCard from './ToolCallCard.vue'
import { useSettingsStore } from '../../stores/settings'

const props = defineProps<{ message: ChatMessage }>()
const settingsStore = useSettingsStore()
const messageRef = ref<HTMLElement | null>(null)
const copiedCodeIndex = ref<number | null>(null)
const copied = ref(false)
const showRawContent = ref(false)

const rawContentViewEnabled = computed(() => settingsStore.settings.rawContentViewEnabled !== false)

/** Show “source” toggle when there is reasoning and/or raw model text differs from visible body. */
const hasRawContent = computed(() => {
  if (!rawContentViewEnabled.value) return false
  if (props.message.reasoning?.trim()) return true
  const raw = props.message.rawContent
  return !!(raw && raw !== props.message.content)
})

watch(rawContentViewEnabled, (on) => {
  if (!on) showRawContent.value = false
})

marked.setOptions({ breaks: true, gfm: true })
const html = computed(() =>
  props.message.content ? marked.parse(props.message.content) as string : ''
)

const isUser = computed(() => props.message.role === 'user')
const isStreaming = computed(() => props.message.status === 'streaming')

/** Final user reply via `response` tool — not shown as a tool card (matches backend: no tool-result row). */
function toolCallBaseName(name: string): string {
  const i = name.indexOf(':')
  return i === -1 ? name : name.slice(0, i)
}

const visibleToolCalls = computed(() =>
  props.message.toolCalls?.filter((tc: ToolCall) => toolCallBaseName(tc.name) !== 'response') ?? []
)

function copy() {
  navigator.clipboard.writeText(props.message.content).then(() => {
    copied.value = true
    setTimeout(() => { copied.value = false }, 2000)
  }).catch(e => console.error(e))
}

function copyCode(index: number, event: MouseEvent) {
  const btn = event.currentTarget as HTMLElement
  const pre = btn.closest('pre')
  if (!pre) return
  
  const code = pre.querySelector('code')
  const text = code?.textContent || pre.textContent || ''
  
  navigator.clipboard.writeText(text).then(() => {
    copiedCodeIndex.value = index
    setTimeout(() => {
      copiedCodeIndex.value = null
    }, 2000)
  }).catch(e => console.error(e))
}

function addCodeCopyButtons() {
  if (!messageRef.value) return
  const pres = messageRef.value.querySelectorAll('pre')
  pres.forEach((pre, index) => {
    if (pre.querySelector('.code-copy-btn')) return
    
    pre.classList.add('group')
    const btn = document.createElement('button')
    btn.className = 'code-copy-btn'
    btn.title = '复制代码'
    btn.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`
    btn.addEventListener('click', (e) => copyCode(index, e as MouseEvent))
    pre.appendChild(btn)
  })
}

onMounted(() => {
  nextTick(addCodeCopyButtons)
})

watch(() => props.message.content, () => {
  nextTick(addCodeCopyButtons)
})
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
        class="block overflow-hidden px-4 py-3 rounded-2xl border break-words"
        :class="isUser
          ? 'w-fit max-w-[80%] bg-primary/15 border-primary/25 text-slate-100'
          : 'w-full max-w-full glass border-white/5'"
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

      <div v-if="visibleToolCalls.length" class="mt-2 space-y-2 w-full">
        <ToolCallCard v-for="tc in visibleToolCalls" :key="tc.id" :tool-call="tc" />
      </div>

      <div v-if="isUser" class="mt-1.5 flex items-center gap-1">
        <button class="p-1.5 rounded hover:bg-white/5 cursor-pointer transition" :class="copied ? 'text-green-400' : 'text-slate-400 hover:text-slate-200'" @click="copy" :title="copied ? '已复制' : '复制'">
          <Check v-if="copied" class="w-3.5 h-3.5" />
          <Copy v-else class="w-3.5 h-3.5" />
        </button>
      </div>

      <div v-if="!isUser && message.status === 'done'" class="mt-1.5 flex items-center gap-1">
        <button class="p-1.5 rounded hover:bg-white/5 cursor-pointer transition" :class="copied ? 'text-green-400' : 'text-slate-400 hover:text-slate-200'" @click="copy" :title="copied ? '已复制' : '复制'">
          <Check v-if="copied" class="w-3.5 h-3.5" />
          <Copy v-else class="w-3.5 h-3.5" />
        </button>
        <button v-if="hasRawContent" class="p-1.5 rounded hover:bg-white/5 cursor-pointer transition" :class="showRawContent ? 'text-primary-cyan' : 'text-slate-400 hover:text-slate-200'" @click="showRawContent = !showRawContent" :title="showRawContent ? '隐藏原始内容' : '查看原始内容'">
          <Code class="w-3.5 h-3.5" />
        </button>
      </div>

      <div
        v-if="!isUser && showRawContent && (message.reasoning?.trim() || message.rawContent)"
        class="mt-2 w-full rounded-xl border border-primary/20 bg-black/30 overflow-hidden"
      >
        <div class="flex items-center justify-between px-3 py-1.5 border-b border-primary/10 bg-primary/5">
          <span class="text-[11px] font-medium text-primary-cyan">推理与原始输出</span>
          <button type="button" class="text-[11px] text-slate-400 hover:text-slate-200 transition" @click="showRawContent = false">收起</button>
        </div>
        <div v-if="message.reasoning?.trim()" class="border-b border-white/10">
          <div class="px-3 pt-2 text-[10px] uppercase tracking-wide text-slate-500">reasoning</div>
          <pre class="max-h-48 overflow-auto p-3 pt-1 text-[11px] leading-relaxed text-slate-400 whitespace-pre-wrap italic">{{ message.reasoning }}</pre>
        </div>
        <div v-if="message.rawContent">
          <div
            v-if="message.reasoning?.trim()"
            class="px-3 pt-2 text-[10px] uppercase tracking-wide text-slate-500"
          >raw</div>
          <pre
            class="max-h-80 overflow-auto p-3 text-[11px] leading-relaxed text-slate-300 whitespace-pre-wrap"
            :class="message.reasoning?.trim() ? 'pt-1' : ''"
          >{{ message.rawContent }}</pre>
        </div>
      </div>
    </div>
  </div>
</template>
