<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { marked } from 'marked'
import { Copy, Check } from 'lucide-vue-next'
import type { MessageStatus, ToolCall } from '../../../../types/chat'
import ToolCallCard from '../../ToolCallCard.vue'
import { useMarkdownCodeCopy } from '../../../../composables/useMarkdownCodeCopy'
import { useMarkdownExternalLinks } from '../../../../composables/useMarkdownExternalLinks'
import { visibleToolCalls, isResponseAssistantMessage, toolCallBaseName } from '../../../../lib/messageTooling'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import { isMessageStreaming } from '../../../../lib/assistantMessageKind'
import ModelThoughtPanels from './ModelThoughtPanels.vue'
import MessageTimeChip from '../MessageTimeChip.vue'

export interface AgentMessageBodyModel {
  thoughts?: string
  toolNamePreview?: string
  responseTextDraft?: string
  reasoning?: string
  content?: string
  rawContent?: string
  contentStreaming?: boolean
  toolCalls?: ToolCall[]
  status: MessageStatus
  createdAt: number
  errorMessage?: string
}

const props = defineProps<{
  body: AgentMessageBodyModel
  messageUi: ResolvedAgentUi
  hideResponse?: boolean
  hideCopy?: boolean
  thoughtsDebugEnabled?: boolean
  generating: boolean
  isActiveGenerationMessage: boolean
}>()

const bodyRef = ref<HTMLElement | null>(null)
const copied = ref(false)

marked.setOptions({ breaks: true, gfm: true })

const isStreaming = computed(() => isMessageStreaming(props.body.status))

const hideStreamingJsonEnvelopeMarkdown = computed(
  () => isStreaming.value && (props.body.content?.trimStart().startsWith('{') ?? false)
)

const isStreamingResponseDraft = computed(() => {
  if (props.hideResponse) return false
  const draft = props.body.responseTextDraft?.trim()
  if (!draft || !isContentStreaming.value) return false
  if (hideStreamingJsonEnvelopeMarkdown.value) return true
  const preview = props.body.toolNamePreview?.trim()
  return !!preview && toolCallBaseName(preview) === 'response'
})

const showMainMarkdownBody = computed(() => {
  if (props.hideResponse) return false
  if (isStreamingResponseDraft.value) return false
  const c = props.body.content?.trim() ?? ''
  if (!c) return false
  return !hideStreamingJsonEnvelopeMarkdown.value
})

const markdownSource = computed(() => {
  if (showMainMarkdownBody.value) return props.body.content ?? ''
  if (hideStreamingJsonEnvelopeMarkdown.value || isStreamingResponseDraft.value)
    return props.body.responseTextDraft ?? ''
  return props.body.content ?? ''
})

const html = computed(() => {
  const src = markdownSource.value
  if (!src.trim()) return ''
  return marked.parse(src) as string
})

const showMdBody = computed(() => !!html.value)

const showStreamingPlaceholderUnderThoughts = computed(
  () =>
    isStreaming.value &&
    !props.hideResponse &&
    (hideStreamingJsonEnvelopeMarkdown.value || isStreamingResponseDraft.value) &&
    !props.body.thoughts?.trim() &&
    !(props.body.responseTextDraft?.trim())
)

const isContentStreaming = computed(() => {
  if (props.body.contentStreaming === false) return false
  if (props.body.contentStreaming === true) return true
  return (
    props.generating &&
    props.isActiveGenerationMessage &&
    isMessageStreaming(props.body.status)
  )
})

const showThoughtsPanel = computed(() => {
  const t = props.body.thoughts?.trim()
  if (!t) return false
  if (isContentStreaming.value) return true
  return props.thoughtsDebugEnabled === true
})

useMarkdownCodeCopy(bodyRef, () => markdownSource.value)
useMarkdownExternalLinks(bodyRef, () => markdownSource.value)

const tools = computed(() =>
  props.messageUi.showToolCalls
    ? visibleToolCalls(
      props.body.toolCalls,
      props.messageUi.hideToolNames,
      props.messageUi.showSidecarToolCalls === true,
      props.messageUi.showNonSidecarToolCalls !== false
    )
    : []
)

const showThoughtPanels = computed(() => showThoughtsPanel.value)

const showReasoningBlock = computed(() =>
  props.messageUi.showReasoning && !!(props.body.reasoning?.trim())
)

const hasBubbleBody = computed(
  () =>
    showMdBody.value ||
    showStreamingPlaceholderUnderThoughts.value ||
    showThoughtPanels.value ||
    props.body.status === 'error'
)

const showCopyButton = computed(() => {
  if (props.hideCopy) return false
  return isResponseAssistantMessage({
    toolCalls: props.body.toolCalls,
    toolNamePreview: props.body.toolNamePreview,
    responseTextDraft: props.body.responseTextDraft,
    content: props.body.content ?? ''
  })
})

const showCopyInBubble = computed(() => showCopyButton.value && showMdBody.value)

// reasoning collapsible state
const reasoningOpen = ref(true)
let reasoningCollapseTimer: ReturnType<typeof setTimeout> | null = null

function clearReasoningCollapseTimer() {
  if (reasoningCollapseTimer) {
    clearTimeout(reasoningCollapseTimer)
    reasoningCollapseTimer = null
  }
}

watch(
  () => props.body.status,
  (status, prevStatus) => {
    if (isMessageStreaming(status)) {
      reasoningOpen.value = true
      clearReasoningCollapseTimer()
      return
    }
    const wasStreaming = prevStatus !== undefined && isMessageStreaming(prevStatus)
    if (wasStreaming) {
      clearReasoningCollapseTimer()
      reasoningCollapseTimer = setTimeout(() => {
        reasoningOpen.value = false
        reasoningCollapseTimer = null
      }, 3000)
    }
  },
  { immediate: true }
)

watch(
  () => props.body.reasoning?.trim() ?? '',
  (h, prev) => {
    if (h && !prev) reasoningOpen.value = true
  }
)

function toggleReasoning() {
  reasoningOpen.value = !reasoningOpen.value
}

function copyBody() {
  const fromMd = markdownSource.value.trim()
  const text = fromMd || props.body.content?.trim() || ''
  void navigator.clipboard.writeText(text).then(() => {
    copied.value = true
    setTimeout(() => {
      copied.value = false
    }, 2000)
  })
}

onUnmounted(() => clearReasoningCollapseTimer())
</script>

<template>
  <div class="w-full max-w-full space-y-2">
    <!-- 推理过程 -->
    <div
      v-if="showReasoningBlock"
      class="w-full rounded-2xl border border-border/60 border-l-2 border-l-accent/40 px-4 py-2.5 bg-muted/10 overflow-hidden"
    >
      <div class="flex items-center gap-2 min-w-0">
        <button
          type="button"
          class="min-w-0 flex-1 text-left flex items-center gap-1.5 cursor-pointer select-none hover:bg-hover transition rounded-md -mx-0.5 px-0.5 sm:-mx-1 sm:px-1"
          :aria-expanded="reasoningOpen"
          @click="toggleReasoning"
        >
          <span class="shrink-0 text-[11px] text-muted font-medium">推理过程</span>
          <span
            class="inline-block w-3 shrink-0 text-muted text-center text-[10px] transition-transform pt-0.5"
            :class="reasoningOpen ? 'rotate-90' : ''"
          >▸</span>
        </button>
        <MessageTimeChip :created-at="body.createdAt" class="shrink-0 self-center" />
      </div>
      <div
        v-if="reasoningOpen"
        class="mt-2 text-[13px] leading-relaxed text-muted whitespace-pre-wrap break-words border-t border-border/30 pt-2"
      >{{ body.reasoning?.trim() }}</div>
    </div>

    <div v-if="hasBubbleBody" class="relative block px-4 py-3 rounded-2xl border break-words panel overflow-x-auto">
      <div
        v-if="!showReasoningBlock"
        class="flex justify-end mb-2 -mt-0.5"
      >
        <MessageTimeChip :created-at="body.createdAt" />
      </div>

      <ModelThoughtPanels
        v-if="showThoughtPanels"
        :xml-thoughts="body.thoughts"
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :is-streaming="isContentStreaming"
      />

      <div
        v-if="showMdBody"
        ref="bodyRef"
        class="md-body"
        :class="showCopyInBubble ? 'pb-6' : ''"
        v-html="html"
      />
      <div
        v-else-if="showStreamingPlaceholderUnderThoughts"
        class="flex items-center text-slate-400 text-sm"
      >
        <span class="typing-dot" />
        <span class="typing-dot" style="animation-delay: 0.2s" />
        <span class="typing-dot" style="animation-delay: 0.4s" />
      </div>

      <div v-if="body.status === 'error'" class="mt-2 flex items-center gap-2 text-xs text-danger">
        {{ body.errorMessage || '生成失败' }}
      </div>

      <button
        v-if="showCopyInBubble"
        type="button"
        class="message-bubble-copy-btn absolute bottom-1.5 left-1.5 z-10"
        :class="copied ? 'text-success' : 'text-muted hover:text-foreground'"
        :title="copied ? '已复制' : '复制'"
        @click="copyBody"
      >
        <Check v-if="copied" class="w-3 h-3" />
        <Copy v-else class="w-3 h-3" />
      </button>
    </div>

    <div v-if="tools.length" class="space-y-2 w-full">
      <ToolCallCard
        v-for="tc in tools"
        :key="tc.id"
        :tool-call="tc"
        :show-tool-call-results="messageUi.showToolCallResults"
      />
    </div>
  </div>
</template>
