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
  headline?: string
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
      props.messageUi.showToolCallResults === true
    )
    : []
)

const CHARS_PER_PIPE = 100
const MAX_HEADLINE_PIPES = 48
const hasHeadline = computed(() => !!(props.body.headline && props.body.headline.trim()))

const streamedCharCount = computed(() => {
  const c = props.body.content?.length ?? 0
  const raw = props.body.rawContent?.length ?? 0
  const thoughtsLen = props.body.thoughts?.length ?? 0
  const toolPreview = props.body.toolNamePreview?.length ?? 0
  const draftLen = props.body.responseTextDraft?.length ?? 0
  const reasoningLen = props.body.reasoning?.length ?? 0
  return Math.max(c, raw, thoughtsLen, toolPreview, draftLen, reasoningLen)
})

const showHeadlineBlock = computed(() => !!(props.body.headline?.trim()))

const isRunInProgress = computed(
  () => isStreaming.value || (props.generating && props.isActiveGenerationMessage)
)

const showHeadlineProgressBar = computed(
  () => !hasHeadline.value && isRunInProgress.value
)

const showThoughtPanels = computed(() => showThoughtsPanel.value)

const hasBubbleBody = computed(
  () =>
    showMdBody.value ||
    showStreamingPlaceholderUnderThoughts.value ||
    showThoughtPanels.value ||
    showHeadlineProgressBar.value ||
    props.body.status === 'error'
)

const headlinePipeBar = computed(() => {
  const n = streamedCharCount.value
  const segments = n <= 0 ? 1 : Math.ceil(n / CHARS_PER_PIPE)
  const pipes = Math.min(MAX_HEADLINE_PIPES, segments)
  return '|'.repeat(pipes)
})

const headlinePipesAtCap = computed(
  () => showHeadlineProgressBar.value && streamedCharCount.value >= CHARS_PER_PIPE * MAX_HEADLINE_PIPES
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

const headlineOpen = ref(true)
let headlineCollapseTimer: ReturnType<typeof setTimeout> | null = null

function clearHeadlineCollapseTimer() {
  if (headlineCollapseTimer) {
    clearTimeout(headlineCollapseTimer)
    headlineCollapseTimer = null
  }
}

watch(
  () => props.body.status,
  (status, prevStatus) => {
    if (isMessageStreaming(status)) {
      headlineOpen.value = true
      clearHeadlineCollapseTimer()
      return
    }
    const wasStreaming = prevStatus !== undefined && isMessageStreaming(prevStatus)
    if (wasStreaming) {
      clearHeadlineCollapseTimer()
      headlineCollapseTimer = setTimeout(() => {
        headlineOpen.value = false
        headlineCollapseTimer = null
      }, 3000)
    }
  },
  { immediate: true }
)

watch(
  () => props.body.headline?.trim() ?? '',
  (h, prev) => {
    if (h && !prev) headlineOpen.value = true
  }
)

function toggleHeadline() {
  headlineOpen.value = !headlineOpen.value
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

onUnmounted(() => clearHeadlineCollapseTimer())
</script>

<template>
  <div class="w-full max-w-full space-y-2">
    <div
      v-if="showHeadlineBlock"
      class="w-full rounded-lg border border-border bg-accent-muted/40 overflow-hidden"
    >
      <div class="flex items-center gap-2 px-1.5 py-1.5 sm:px-2 min-w-0">
        <button
          type="button"
          class="min-w-0 flex-1 text-left flex items-center gap-1.5 cursor-pointer select-none hover:bg-hover transition rounded-md -mx-0.5 px-0.5 sm:-mx-1 sm:px-1"
          :aria-expanded="headlineOpen"
          @click="toggleHeadline"
        >
          <span
            class="inline-block w-3.5 shrink-0 text-accent text-center text-[10px] transition-transform pt-0.5"
            :class="headlineOpen ? 'rotate-90' : ''"
          >▸</span>
          <span
            class="min-w-0 flex-1 text-[12px] sm:text-[13px] font-medium text-foreground leading-tight tracking-tight"
            :class="headlineOpen ? 'whitespace-pre-wrap' : 'line-clamp-2 overflow-hidden'"
          >{{ body.headline?.trim() }}</span>
        </button>
        <MessageTimeChip :created-at="body.createdAt" class="shrink-0 self-center" />
      </div>
    </div>

    <div v-if="hasBubbleBody" class="relative block px-4 py-3 rounded-2xl border break-words panel overflow-x-auto">
      <div
        v-if="!hasHeadline && !showHeadlineProgressBar"
        class="flex justify-end mb-2 -mt-0.5"
      >
        <MessageTimeChip :created-at="body.createdAt" />
      </div>

      <div
        v-if="showHeadlineProgressBar"
        class="mb-3 flex items-start gap-2 min-w-0"
      >
        <div
          class="flex-1 min-w-0 font-mono text-[13px] leading-tight tracking-[0.06em] text-accent/80 min-h-[1.125rem] select-none break-all whitespace-pre-wrap"
          role="status"
          aria-live="polite"
          :class="headlinePipesAtCap ? 'animate-pulse' : ''"
        >
          {{ headlinePipeBar }}
        </div>
        <MessageTimeChip :created-at="body.createdAt" class="shrink-0 pt-0.5" />
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
