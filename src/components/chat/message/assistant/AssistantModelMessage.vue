<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { marked } from 'marked'
import { Copy, Check, Code, Camera } from 'lucide-vue-next'
import type { ChatMessage, ComputerAnnotatedPreview } from '../../../../types/chat'
import ToolCallCard from '../../ToolCallCard.vue'
import { useSettingsStore } from '../../../../stores/settings'
import { useChatStore } from '../../../../stores/chat'
import { previewComputerAnnotatedScreen, previewComputerRoundScreen } from '../../../../lib/api'
import { isTauriRuntime } from '../../../../lib/runtime'
import { useMarkdownCodeCopy } from '../../../../composables/useMarkdownCodeCopy'
import { visibleToolCalls, isResponseAssistantMessage, toolCallBaseName } from '../../../../lib/messageTooling'
import { shouldShowSubAgentTrace } from '../../../../lib/agentUi'
import { useAgentsCatalog, uiForMessageAgent } from '../../../../composables/useAgentUi'
import { isMessageStreaming } from '../../../../lib/assistantMessageKind'
import { showAnnotatedScreenAction } from '../../../../lib/computerMessageContext'
import ModelThoughtPanels from './ModelThoughtPanels.vue'
import RawWirePanel from './RawWirePanel.vue'
import ScreenPreviewModal from './ScreenPreviewModal.vue'
import MessageTimeChip from '../MessageTimeChip.vue'

const props = defineProps<{ message: ChatMessage }>()

const settingsStore = useSettingsStore()
const agentsCatalog = useAgentsCatalog()
const messageUi = computed(() =>
  uiForMessageAgent(
    props.message.agentId,
    props.message.agentName,
    settingsStore.settings,
    agentsCatalog.value
  )
)

const showSubAgentTrace = computed(() =>
  shouldShowSubAgentTrace(
    messageUi.value,
    props.message.agentTrace,
    settingsStore.settings,
    props.message.agentId
  )
)
const bodyRef = ref<HTMLElement | null>(null)
const copied = ref(false)
const showRawWire = ref(false)
const modalOpen = ref(false)
const screenLoading = ref(false)
const screenPreview = ref<ComputerAnnotatedPreview | null>(null)
const screenError = ref<string | null>(null)

marked.setOptions({ breaks: true, gfm: true })

const isStreaming = computed(() => isMessageStreaming(props.message.status))

/** `json_object` 回合里正文通道是整段 JSON；流式时主区勿当 Markdown 渲染，避免满屏原始 JSON。 */
const hideStreamingJsonEnvelopeMarkdown = computed(
  () => isStreaming.value && (props.message.content?.trimStart().startsWith('{') ?? false)
)

/** 流式 `response` 预览：子 Agent 等场景 JSON 走 partial，不写入 `message.content`。 */
const isStreamingResponseDraft = computed(() => {
  const draft = props.message.responseTextDraft?.trim()
  if (!draft || !isContentStreaming.value) return false
  if (hideStreamingJsonEnvelopeMarkdown.value) return true
  const preview = props.message.toolNamePreview?.trim()
  return !!preview && toolCallBaseName(preview) === 'response'
})

/** 主气泡 Markdown：流式 JSON 信封阶段不用原始 `content` 渲染；收尾后 `message_end` 会换成 `extract_user_visible_content` 结果。 */
const showMainMarkdownBody = computed(() => {
  if (isStreamingResponseDraft.value) return false
  const c = props.message.content?.trim() ?? ''
  if (!c) return false
  return !hideStreamingJsonEnvelopeMarkdown.value
})

/** 主气泡 Markdown 源码：收尾后为 `content`；流式 JSON / response 阶段为 `responseTextDraft`（`response.text`）。 */
const markdownSource = computed(() => {
  if (showMainMarkdownBody.value) return props.message.content ?? ''
  if (hideStreamingJsonEnvelopeMarkdown.value || isStreamingResponseDraft.value)
    return props.message.responseTextDraft ?? ''
  return props.message.content ?? ''
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
    (hideStreamingJsonEnvelopeMarkdown.value || isStreamingResponseDraft.value) &&
    !props.message.thoughts?.trim() &&
    !(props.message.responseTextDraft?.trim())
)

const thoughtsDebugEnabled = computed(() => messageUi.value.showThoughts)

const showThoughtsPanel = computed(() => {
  const t = props.message.thoughts?.trim()
  if (!t) return false
  if (isContentStreaming.value) return true
  return thoughtsDebugEnabled.value
})

useMarkdownCodeCopy(bodyRef, () => markdownSource.value)

const rawContentViewEnabled = computed(() => settingsStore.settings.rawContentViewEnabled === true)

const hasRawWire = computed(() => {
  if (!rawContentViewEnabled.value) return false
  const raw = props.message.rawContent
  const reasoning = props.message.reasoning?.trim() ?? ''
  const hasReasoning = reasoning.length > 0
  const rawDiffersFromBody = !!(raw && raw !== props.message.content)
  return rawDiffersFromBody || hasReasoning
})

const showCamera = computed(() => {
  if (!isTauriRuntime()) return false
  return showAnnotatedScreenAction(props.message, {
    agentMode: settingsStore.settings.agentMode,
    leadAgentId: settingsStore.settings.leadAgentId ?? '',
    annotatedScreenViewEnabled: settingsStore.settings.computerAnnotatedScreenViewEnabled === true
  })
})

const tools = computed(() =>
  messageUi.value.showToolCalls
    ? visibleToolCalls(props.message.toolCalls, messageUi.value.hideToolNames)
    : []
)

/** headline 未出现时，用流式字符数推进竖线 `|`，按段向上取整多一根；无百分比。 */
const CHARS_PER_PIPE = 100
const MAX_HEADLINE_PIPES = 48

const hasHeadline = computed(() => !!(props.message.headline && props.message.headline.trim()))

/** 无 headline 时竖线进度：按「整段流式输出」体量推进（含 API reasoning 字符数）。 */
const streamedCharCount = computed(() => {
  const c = props.message.content?.length ?? 0
  const raw = props.message.rawContent?.length ?? 0
  const thoughtsLen = props.message.thoughts?.length ?? 0
  const toolPreview = props.message.toolNamePreview?.length ?? 0
  const draftLen = props.message.responseTextDraft?.length ?? 0
  const reasoningLen = props.message.reasoning?.length ?? 0
  return Math.max(c, raw, thoughtsLen, toolPreview, draftLen, reasoningLen)
})

const showHeadlineBlock = computed(() => !!(props.message.headline?.trim()))

const chatStore = useChatStore()
const { generating, activeGeneratingMessageId } = storeToRefs(chatStore)

const isActiveGenerationMessage = computed(
  () => props.message.id === activeGeneratingMessageId.value
)

/** 当前 LLM 回合是否在流式输出（`message_end` 后为 false；多轮工具间隙也为 false）。 */
const isContentStreaming = computed(() => {
  if (props.message.contentStreaming === false) return false
  if (props.message.contentStreaming === true) return true
  // 兼容缺字段或热更新间隙：仅当前正在生成的助手行且 status 仍为 streaming/pending
  return (
    generating.value &&
    isActiveGenerationMessage.value &&
    isMessageStreaming(props.message.status)
  )
})

/** Copy / screenshot / raw-wire row: hide only while this row is actively streaming in the current run. */
const showMessageActions = computed(() => {
  if (generating.value && isActiveGenerationMessage.value) return false
  if (props.message.status === 'pending') return false
  return true
})

const showCopyButton = computed(
  () => showMessageActions.value && isResponseAssistantMessage(props.message)
)

const showActionBar = computed(
  () => showCamera.value || (showMessageActions.value && hasRawWire.value)
)

const showCopyInBubble = computed(() => showCopyButton.value && showMdBody.value)

const isRunInProgress = computed(
  () =>
    isStreaming.value ||
    (generating.value && isActiveGenerationMessage.value)
)

const showHeadlineProgressBar = computed(
  () => !hasHeadline.value && isRunInProgress.value
)

const showThoughtPanels = computed(
  () =>
    showThoughtsPanel.value ||
    (showSubAgentTrace.value &&
      ((props.message.agentTrace?.length ?? 0) > 0 ||
        (props.message.supervisorPlanTasks?.length ?? 0) > 0))
)

const hasBubbleBody = computed(
  () =>
    showMdBody.value ||
    showStreamingPlaceholderUnderThoughts.value ||
    showThoughtPanels.value ||
    showHeadlineProgressBar.value ||
    props.message.status === 'error'
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

watch(rawContentViewEnabled, on => {
  if (!on) showRawWire.value = false
})

/** 整条助手消息的标题：流式展开，结束后 3s 收起为两行摘要（与原先 headline 条行为一致）。 */
const headlineOpen = ref(true)
let headlineCollapseTimer: ReturnType<typeof setTimeout> | null = null

function clearHeadlineCollapseTimer() {
  if (headlineCollapseTimer) {
    clearTimeout(headlineCollapseTimer)
    headlineCollapseTimer = null
  }
}

watch(
  () => props.message.status,
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
  () => props.message.headline?.trim() ?? '',
  (h, prev) => {
    if (h && !prev) headlineOpen.value = true
  }
)

function toggleHeadline() {
  headlineOpen.value = !headlineOpen.value
}

function copyBody() {
  const fromMd = markdownSource.value.trim()
  const text =
    fromMd ||
    props.message.rawContent?.trim() ||
    props.message.content?.trim() ||
    ''
  void navigator.clipboard.writeText(text).then(() => {
    copied.value = true
    setTimeout(() => {
      copied.value = false
    }, 2000)
  })
}

async function openScreenPreview() {
  if (!isTauriRuntime()) return
  screenLoading.value = true
  screenError.value = null
  screenPreview.value = null
  modalOpen.value = true
  try {
    const rel = props.message.computerRoundScreenRelPath?.trim()
    if (rel) {
      screenPreview.value = await previewComputerRoundScreen(rel)
    } else {
      const cid = chatStore.currentId
      if (cid) {
        screenPreview.value = await previewComputerAnnotatedScreen(cid)
      } else {
        screenError.value = '无法获取当前会话 ID'
      }
    }
  } catch (e: unknown) {
    screenError.value = e instanceof Error ? e.message : String(e)
  } finally {
    screenLoading.value = false
  }
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
          >{{ message.headline?.trim() }}</span>
        </button>
        <MessageTimeChip :created-at="message.createdAt" class="shrink-0 self-center" />
      </div>
    </div>

    <div v-if="hasBubbleBody" class="relative block px-4 py-3 rounded-2xl border break-words panel overflow-x-auto">
      <div
        v-if="!hasHeadline && !showHeadlineProgressBar"
        class="flex justify-end mb-2 -mt-0.5"
      >
        <MessageTimeChip :created-at="message.createdAt" />
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
        <MessageTimeChip :created-at="message.createdAt" class="shrink-0 pt-0.5" />
      </div>

      <ModelThoughtPanels
        v-if="showThoughtPanels"
        :xml-thoughts="message.thoughts"
        :agent-trace="message.agentTrace"
        :plan-tasks="message.supervisorPlanTasks"
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :show-sub-agent-trace="showSubAgentTrace"
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

      <div v-if="message.status === 'error'" class="mt-2 flex items-center gap-2 text-xs text-danger">
        {{ message.errorMessage || '生成失败' }}
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
      <ToolCallCard v-for="tc in tools" :key="tc.id" :tool-call="tc" />
    </div>

    <div v-if="showActionBar" class="flex items-center gap-1 w-full min-w-0">
      <button
        v-if="showCamera"
        type="button"
        class="message-action-btn text-muted hover:text-info disabled:opacity-40 disabled:cursor-wait"
        :disabled="screenLoading"
        title="查看本轮已注入模型的标注桌面图（缓存）"
        @click="openScreenPreview"
      >
        <Camera class="w-3.5 h-3.5" />
      </button>
      <button
        v-if="hasRawWire"
        class="message-action-btn"
        :class="showRawWire ? 'text-accent' : 'text-muted hover:text-foreground'"
        :title="showRawWire ? '隐藏原始内容' : '查看原始内容'"
        @click="showRawWire = !showRawWire"
      >
        <Code class="w-3.5 h-3.5" />
      </button>
    </div>

    <RawWirePanel
      v-if="showRawWire && (message.rawContent || (message.reasoning && message.reasoning.trim()))"
      :reasoning="message.reasoning"
      :raw-content="message.rawContent"
      @close="showRawWire = false"
    />

    <ScreenPreviewModal
      v-model:open="modalOpen"
      :loading="screenLoading"
      :preview="screenPreview"
      :error="screenError"
    />
  </div>
</template>
