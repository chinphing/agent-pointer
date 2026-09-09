<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { parseMarkdown } from '../../../../lib/markdownConfig'
import { useThrottledMarkdown } from '../../../../composables/useThrottledMarkdown'
import type { MessageStatus, ToolCall, ChatMessage } from '../../../../types/chat'
import { useMarkdownCodeCopy } from '../../../../composables/useMarkdownCodeCopy'
import { useMarkdownCharts } from '../../../../composables/useMarkdownCharts'
import { useMarkdownSvgs } from '../../../../composables/useMarkdownSvgs'
import { useMarkdownMermaid } from '../../../../composables/useMarkdownMermaid'
import { useMarkdownExternalLinks } from '../../../../composables/useMarkdownExternalLinks'
import { isCollapsedSurfaceToolCall, visibleToolCalls, toolCallBaseName } from '../../../../lib/messageTooling'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import { isGenerationCancelledMessage, isMessageStreaming } from '../../../../lib/assistantMessageKind'
import ModelThoughtPanels from './ModelThoughtPanels.vue'
import ToolMessageSegment from './ToolMessageSegment.vue'
import AssistantMessageDebugChrome from './AssistantMessageDebugChrome.vue'
import { assistantReplyMediaForRender } from '../../../../lib/messageNormalizer'
import ChatMessageMediaGallery from '../ChatMessageMediaGallery.vue'
import { shouldShowThinkingIndicator } from '../../../../lib/thinkingIndicator'
import { isBalanceExhaustedMessage, openPlatformBillingPage } from '../../../../lib/platformUrls'
import { usePlatformAuthStore } from '../../../../stores/platformAuth'

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
  /** Collapsed turn: reply body only; hide process tools / thoughts / reasoning. */
  contentOnly?: boolean
}>()

const bodyRef = ref<HTMLElement | null>(null)
const platformAuth = usePlatformAuthStore()

const showBalanceRecharge = computed(
  () =>
    props.body.status === 'error' &&
    !platformAuth.isStandalone &&
    isBalanceExhaustedMessage(props.body.errorMessage)
)

const isCancelled = computed(
  () =>
    props.body.status === 'cancelled' ||
    (props.body.status === 'error' &&
      isGenerationCancelledMessage(props.body.errorMessage ?? ''))
)

async function onOpenBilling() {
  try {
    await openPlatformBillingPage()
  } catch (e) {
    console.warn('[chat] open billing page failed', e)
  }
}

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

const replyMediaAttachments = computed(() =>
  assistantReplyMediaForRender(props.leadMessage)
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

const markdownSource = computed(() => {
  if (props.hideResponse) {
    const draft = props.body.responseTextDraft?.trim()
    if (draft && isContentStreaming.value) return draft
    return ''
  }
  let raw = ''
  if (showMainMarkdownBody.value) raw = props.body.content ?? ''
  else if (hideStreamingJsonEnvelopeMarkdown.value || isStreamingResponseDraft.value)
    raw = props.body.responseTextDraft ?? ''
  else raw = props.body.content ?? ''
  return raw
})

const html = useThrottledMarkdown(
  () => markdownSource.value,
  () => isContentStreaming.value,
  src =>
    parseMarkdown(src, {
      streamingCharts: isContentStreaming.value,
      streamingSvgs: isContentStreaming.value,
      streamingMermaid: isContentStreaming.value,
    }),
  { longSourceThreshold: 8000, longStreamingInterval: 250 }
)

const showMdBody = computed(() => !!html.value)

const showStreamingPlaceholderUnderThoughts = computed(
  () =>
    isStreaming.value &&
    !props.hideResponse &&
    (hideStreamingJsonEnvelopeMarkdown.value || isStreamingResponseDraft.value) &&
    !props.body.thoughts?.trim() &&
    !(props.body.responseTextDraft?.trim())
)

/** JSON `thoughts`, or API `reasoning` in debug when Computer has no thoughts field. */
const thoughtsPanelText = computed(() => {
  const thoughts = props.body.thoughts?.trim()
  if (thoughts) return thoughts
  if (props.thoughtsDebugEnabled && props.messageUi.showReasoning) {
    return props.body.reasoning?.trim() ?? ''
  }
  return ''
})

const reasoningShownInThoughtsPanel = computed(
  () =>
    props.thoughtsDebugEnabled &&
    props.messageUi.showReasoning &&
    !props.body.thoughts?.trim() &&
    !!(props.body.reasoning?.trim())
)

const showThoughtsPanel = computed(() => {
  const t = thoughtsPanelText.value
  if (!t) return false
  if (isContentStreaming.value) return true
  return props.thoughtsDebugEnabled === true
})

useMarkdownCodeCopy(bodyRef, () => html.value)
useMarkdownCharts(bodyRef, () => html.value, {
  isStreaming: () => isContentStreaming.value,
})
useMarkdownSvgs(bodyRef, () => html.value, {
  isStreaming: () => isContentStreaming.value,
})
useMarkdownMermaid(bodyRef, () => html.value, {
  isStreaming: () => isContentStreaming.value,
})
useMarkdownExternalLinks(bodyRef, () => html.value)

const tools = computed(() => {
  if (!props.messageUi.showToolCalls) return []
  const visible = visibleToolCalls(
    props.body.toolCalls,
    props.messageUi.hideToolNames,
    props.messageUi.showSidecarToolCalls === true,
    props.messageUi.showNonSidecarToolCalls !== false
  )
  return props.contentOnly ? visible.filter(isCollapsedSurfaceToolCall) : visible
})

const footerMessage = computed((): ChatMessage | undefined => {
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

const leadToolCalls = computed(() => tools.value)

const trailingToolSegments = computed(() => {
  const segments: {
    message: ChatMessage
    toolCalls: ToolCall[]
  }[] = []

  for (const group of props.trailingToolGroups ?? []) {
    const visible = trailingToolsForGroup(group)
    if (visible.length === 0) continue
    segments.push({
      message: group.message,
      toolCalls: visible
    })
  }

  return segments
})

const mergedProcessTools = computed(() => [
  ...leadToolCalls.value,
  ...trailingToolSegments.value.flatMap(segment => segment.toolCalls)
])

const processToolsHostMessage = computed((): ChatMessage | undefined => {
  const trailingGroups = props.trailingToolGroups ?? []
  for (let i = trailingGroups.length - 1; i >= 0; i -= 1) {
    const message = trailingGroups[i]!.message
    if (isMessageStreaming(message.status)) return message
  }
  const trailing = trailingToolSegments.value
  if (leadToolCalls.value.length && footerMessage.value) return footerMessage.value
  return trailing[trailing.length - 1]?.message ?? footerMessage.value
})

const showLeadUnit = computed(
  () =>
    showReasoningBlock.value
    || hasMainBody.value
    || leadToolCalls.value.length > 0
    || isCancelled.value
    || showThinkingIndicator.value
)

const processToolsCompactTop = computed(() => {
  if (!showLeadUnit.value) return true
  return !hasMainBody.value && !showReasoningBlock.value
})

const showProcessToolSlot = computed(
  () =>
    !!processToolsHostMessage.value
    && (mergedProcessTools.value.length > 0 || showThinkingIndicator.value)
)

const showToolSegments = computed(
  () => showLeadUnit.value || mergedProcessTools.value.length > 0 || showThinkingIndicator.value
)

function trailingToolsForGroup(group: { toolCalls: ToolCall[]; message: ChatMessage }): ToolCall[] {
  if (!props.messageUi.showToolCalls) return []
  const visible = visibleToolCalls(
    group.toolCalls,
    props.messageUi.hideToolNames,
    props.messageUi.showSidecarToolCalls === true,
    props.messageUi.showNonSidecarToolCalls !== false
  )
  return props.contentOnly ? visible.filter(isCollapsedSurfaceToolCall) : visible
}

const showThoughtPanels = computed(() => !props.contentOnly && showThoughtsPanel.value)

const isRunInProgress = computed(
  () => isStreaming.value || (props.generating && props.isActiveGenerationMessage)
)

const showReasoningBlock = computed(
  () =>
    !props.contentOnly &&
    props.messageUi.showReasoning &&
    !!(props.body.reasoning?.trim()) &&
    !reasoningShownInThoughtsPanel.value
)

const reasoningDisplayText = computed(() => props.body.reasoning ?? '')

const showThinkingIndicator = computed(() =>
  !props.contentOnly &&
  shouldShowThinkingIndicator({
    runInProgress: isRunInProgress.value,
    markdownBodyVisible: showMdBody.value,
    thoughtsPanelVisible: showThoughtPanels.value,
    reasoningVisible: showReasoningBlock.value,
    extraVisibleTools: mergedProcessTools.value.length > 0,
    body: props.body
  })
)

const hasMainBody = computed(
  () =>
    showMdBody.value ||
    replyMediaAttachments.value.length > 0 ||
    showStreamingPlaceholderUnderThoughts.value ||
    showThoughtPanels.value ||
    props.body.status === 'error'
)

const showCopyButton = computed(() => {
  if (props.hideCopy) return false
  if (props.toolOnly) return false
  if (!copyText.value.trim()) return false
  return showMdBody.value || props.body.status === 'error'
})

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
    if (wasStreaming && !props.thoughtsDebugEnabled) {
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
  () => props.body.reasoning?.length ?? 0,
  (len, prevLen) => {
    if (len > 0 && (prevLen ?? 0) === 0) reasoningOpen.value = true
  }
)

function toggleReasoning() {
  reasoningOpen.value = !reasoningOpen.value
}

onUnmounted(() => clearReasoningCollapseTimer())
</script>

<template>
  <div class="w-full max-w-full space-y-0">
    <div v-if="showToolSegments" class="tool-segments">
      <div
        v-if="showLeadUnit"
        class="assistant-message-unit w-full"
      >
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
          >{{ reasoningDisplayText }}</div>
        </div>

        <div v-if="hasMainBody" class="chat-hover-root message-stamp-host w-full min-w-0">
          <div class="break-words overflow-x-hidden">
          <ModelThoughtPanels
            v-if="showThoughtPanels"
            :xml-thoughts="thoughtsPanelText"
            :thoughts-debug-enabled="thoughtsDebugEnabled"
            :is-streaming="isContentStreaming"
          />

          <div
            v-if="showMdBody"
            ref="bodyRef"
            class="md-body md-body-flow px-3"
            v-html="html"
          />

          <ChatMessageMediaGallery
            v-if="replyMediaAttachments.length"
            class="px-3 mt-2"
            :attachments="replyMediaAttachments"
          />
          <div
            v-else-if="showStreamingPlaceholderUnderThoughts && !showThinkingIndicator"
            class="flex items-center text-muted text-sm px-3"
          >
            <span class="typing-dot" />
            <span class="typing-dot" style="animation-delay: 0.2s" />
            <span class="typing-dot" style="animation-delay: 0.4s" />
          </div>

          <div v-if="body.status === 'error'" class="mt-2 px-3 text-xs text-danger">
            <div class="flex items-center gap-2">
              {{
                showBalanceRecharge
                  ? '账户余额已用尽，充值后可继续对话'
                  : body.errorMessage || '生成失败'
              }}
            </div>
            <button
              v-if="showBalanceRecharge"
              type="button"
              class="mt-1.5 inline-flex rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-accent-foreground hover:opacity-90 cursor-pointer"
              @click="onOpenBilling"
            >
              去充值
            </button>
          </div>
          </div>

          <AssistantMessageDebugChrome
            v-if="footerMessage"
            :message="footerMessage"
            :copy-text="copyText"
            :show-copy="showCopyButton || undefined"
            :generating="generating"
            :is-active-generation-message="isActiveGenerationMessage"
          />
        </div>

        <ToolMessageSegment
          v-if="showProcessToolSlot && processToolsHostMessage"
          :message="processToolsHostMessage"
          :tool-calls="mergedProcessTools"
          :message-ui="messageUi"
          :compact-top="processToolsCompactTop"
        >
          <template #after-tool="slotProps">
            <slot
              name="after-tool"
              v-bind="slotProps"
            />
          </template>
        </ToolMessageSegment>

        <!-- Below tools so stop is visible on tool-only turns (same muted inline as empty cancel). -->
        <div
          v-if="isCancelled"
          class="tool-call-row px-3"
          role="status"
        >
          <div class="py-0.5 inline-flex items-center text-[11px] text-muted">
            已停止生成
          </div>
        </div>
      </div>

      <ToolMessageSegment
        v-else-if="showProcessToolSlot && processToolsHostMessage"
        :message="processToolsHostMessage"
        :tool-calls="mergedProcessTools"
        :message-ui="messageUi"
        compact-top
      >
        <template #after-tool="slotProps">
          <slot
            name="after-tool"
            v-bind="slotProps"
          />
        </template>
      </ToolMessageSegment>
    </div>
  </div>
</template>
