<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { marked } from 'marked'
import type { MessageStatus, ToolCall, ChatMessage } from '../../../../types/chat'
import { useMarkdownCodeCopy } from '../../../../composables/useMarkdownCodeCopy'
import { useMarkdownExternalLinks } from '../../../../composables/useMarkdownExternalLinks'
import { visibleToolCalls, isResponseAssistantMessage, toolCallBaseName } from '../../../../lib/messageTooling'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import { isMessageStreaming } from '../../../../lib/assistantMessageKind'
import ModelThoughtPanels from './ModelThoughtPanels.vue'
import MessageFooterActions from '../MessageFooterActions.vue'
import ToolMessageSegment from './ToolMessageSegment.vue'

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
  leadMessage?: ChatMessage
  messageUi: ResolvedAgentUi
  hideResponse?: boolean
  hideCopy?: boolean
  thoughtsDebugEnabled?: boolean
  generating: boolean
  isActiveGenerationMessage: boolean
  toolOnly?: boolean
  trailingToolGroups?: { id: string; toolCalls: ToolCall[]; message: ChatMessage }[]
}>()

const bodyRef = ref<HTMLElement | null>(null)

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

const leadSegmentMessage = computed((): ChatMessage => {
  if (props.leadMessage) return props.leadMessage
  return {
    id: `body-${props.body.createdAt}`,
    role: 'assistant',
    content: props.body.content ?? '',
    toolCalls: props.body.toolCalls,
    createdAt: props.body.createdAt,
    status: props.body.status,
    reasoning: props.body.reasoning,
    rawContent: props.body.rawContent,
    toolNamePreview: props.body.toolNamePreview,
    responseTextDraft: props.body.responseTextDraft
  } as ChatMessage
})

const toolSegments = computed(() => {
  const segments: {
    message: ChatMessage
    toolCalls: ToolCall[]
    compactTop: boolean
    copyText?: string
  }[] = []

  if (tools.value.length > 0) {
    segments.push({
      message: leadSegmentMessage.value,
      toolCalls: tools.value,
      compactTop: !hasMainBody.value,
      copyText: hasMainBody.value ? copyText.value : undefined
    })
  }

  for (const group of props.trailingToolGroups ?? []) {
    segments.push({
      message: group.message,
      toolCalls: trailingToolsForGroup(group),
      compactTop: segments.length > 0 || hasMainBody.value
    })
  }

  return segments
})

const hasTools = computed(() => toolSegments.value.length > 0)

function trailingToolsForGroup(group: { toolCalls: ToolCall[]; message: ChatMessage }): ToolCall[] {
  return props.messageUi.showToolCalls
    ? visibleToolCalls(
      group.toolCalls,
      props.messageUi.hideToolNames,
      props.messageUi.showSidecarToolCalls === true,
      props.messageUi.showNonSidecarToolCalls !== false
    )
    : []
}

const showThoughtPanels = computed(() => showThoughtsPanel.value)

const showReasoningBlock = computed(() =>
  props.messageUi.showReasoning && !!(props.body.reasoning?.trim())
)

const hasMainBody = computed(
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

const showTextFooter = computed(
  () => !isStreaming.value && hasMainBody.value && tools.value.length === 0
)

const copyText = computed(() => {
  const fromMd = markdownSource.value.trim()
  return fromMd || props.body.content?.trim() || ''
})

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

onUnmounted(() => clearReasoningCollapseTimer())
</script>

<template>
  <div class="w-full max-w-full" :class="toolOnly ? 'space-y-0' : 'space-y-2'">
    <!-- 推理过程 -->
    <div
      v-if="showReasoningBlock"
      class="w-full rounded-2xl border border-border/50 border-l-2 border-l-accent/40 px-3 py-2.5 bg-muted/10 overflow-hidden"
    >
      <button
        type="button"
        class="w-full min-w-0 text-left flex items-center gap-1.5 cursor-pointer select-none hover:bg-hover transition rounded-md -mx-0.5 px-0.5"
        :aria-expanded="reasoningOpen"
        @click="toggleReasoning"
      >
        <span class="shrink-0 text-[11px] text-muted font-medium">推理过程</span>
        <span
          class="inline-block w-3 shrink-0 text-muted text-center text-[10px] transition-transform pt-0.5"
          :class="reasoningOpen ? 'rotate-90' : ''"
        >▸</span>
      </button>
      <div
        v-if="reasoningOpen"
        class="mt-2 text-[13px] leading-relaxed text-muted whitespace-pre-wrap break-words border-t border-border/30 pt-2"
      >{{ body.reasoning?.trim() }}</div>
    </div>

    <div v-if="hasMainBody" class="chat-hover-root relative w-full break-words overflow-x-auto">
      <ModelThoughtPanels
        v-if="showThoughtPanels"
        :xml-thoughts="body.thoughts"
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :is-streaming="isContentStreaming"
      />

      <div
        v-if="showMdBody"
        ref="bodyRef"
        class="md-body md-body-flow px-3"
        v-html="html"
      />
      <div
        v-else-if="showStreamingPlaceholderUnderThoughts"
        class="flex items-center text-muted text-sm px-3"
      >
        <span class="typing-dot" />
        <span class="typing-dot" style="animation-delay: 0.2s" />
        <span class="typing-dot" style="animation-delay: 0.4s" />
      </div>

      <div v-if="body.status === 'error'" class="mt-2 flex items-center gap-2 text-xs text-danger px-3">
        {{ body.errorMessage || '生成失败' }}
      </div>

      <MessageFooterActions
        v-if="showTextFooter"
        class="px-3 !mt-0"
        :created-at="body.createdAt"
        :copy-text="copyText"
        :show-copy="showCopyButton"
      />
    </div>

    <div v-if="hasTools" class="tool-segments">
      <ToolMessageSegment
        v-for="segment in toolSegments"
        :key="segment.message.id"
        :message="segment.message"
        :tool-calls="segment.toolCalls"
        :message-ui="messageUi"
        :compact-top="segment.compactTop"
        :copy-text="segment.copyText"
      />
    </div>
  </div>
</template>
