<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { Code, Camera } from 'lucide-vue-next'
import type { ChatMessage, ComputerAnnotatedPreview, ToolCall } from '../../../../types/chat'
import { useSettingsStore } from '../../../../stores/settings'
import { useChatStore } from '../../../../stores/chat'
import { previewComputerAnnotatedScreen, previewComputerRoundScreen } from '../../../../lib/api'
import { isMessageStreaming } from '../../../../lib/assistantMessageKind'
import { showAnnotatedScreenAction, messageHasComputerTools } from '../../../../lib/computerMessageContext'
import { isResponseAssistantMessage, toolCallBaseName } from '../../../../lib/messageTooling'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import ToolCallList from '../../ToolCallList.vue'
import MessageFooterActions from '../MessageFooterActions.vue'
import RawWirePanel from './RawWirePanel.vue'
import ScreenPreviewModal from './ScreenPreviewModal.vue'

const props = defineProps<{
  message: ChatMessage
  toolCalls: ToolCall[]
  messageUi: ResolvedAgentUi
  compactTop?: boolean
  copyText?: string
}>()

const settingsStore = useSettingsStore()
const chatStore = useChatStore()
const { generating, activeGeneratingMessageId } = storeToRefs(chatStore)

const showRawWire = ref(false)
const modalOpen = ref(false)
const screenLoading = ref(false)
const screenPreview = ref<ComputerAnnotatedPreview | null>(null)
const screenError = ref<string | null>(null)

const rawContentViewEnabled = computed(() => settingsStore.settings.rawContentViewEnabled === true)

function formatToolArgs(raw: string | undefined): string {
  const text = (raw ?? '').trim()
  if (!text) return '(empty)'
  try {
    const parsed = JSON.parse(text)
    return JSON.stringify(parsed, null, 2)
  } catch {
    return text
  }
}

const toolRawArgs = computed(() => {
  if (!props.message.toolCalls?.length) return ''
  return props.message.toolCalls
    .filter(tc => toolCallBaseName(tc.name) !== 'response')
    .map(tc => {
      const args = formatToolArgs(tc.arguments)
      return `[tool:${tc.name} id:${tc.id}]\n${args}`
    })
    .join('\n\n')
})

const screenRelPath = computed(() => props.message.computerRoundScreenRelPath?.trim() ?? '')

const rawWireReasoning = computed(() => props.message.reasoning?.trim() || undefined)

const rawWireContent = computed(() => {
  const raw = props.message.rawContent?.trim() ?? ''
  if (raw && raw !== (props.message.content?.trim() ?? '')) return raw
  return undefined
})

const hasRawWire = computed(() => {
  if (!rawContentViewEnabled.value) return false
  return !!(rawWireContent.value?.trim() || rawWireReasoning.value?.trim() || toolRawArgs.value.trim())
})

const showCamera = computed(() => {
  if (settingsStore.settings.computerAnnotatedScreenViewEnabled !== true) return false
  if (screenRelPath.value) return true
  const ctx = {
    agentMode: settingsStore.settings.agentMode,
    leadAgentId: settingsStore.settings.leadAgentId ?? '',
    annotatedScreenViewEnabled: true
  }
  return (
    showAnnotatedScreenAction(props.message, ctx)
    || messageHasComputerTools(props.message)
    || props.message.agentId === 'computer'
  )
})

const isStreaming = computed(() => isMessageStreaming(props.message.status))

const isActiveForMessage = computed(
  () => props.message.id === activeGeneratingMessageId.value
)

const showMessageActions = computed(() => {
  if (generating.value && isActiveForMessage.value) return false
  if (props.message.status === 'pending') return false
  return true
})

const showDebugActions = computed(
  () => showMessageActions.value && (showCamera.value || hasRawWire.value)
)

const showFooter = computed(
  () => !isStreaming.value && (props.toolCalls.length > 0 || showDebugActions.value)
)

const showCopyButton = computed(() =>
  isResponseAssistantMessage({
    toolCalls: props.message.toolCalls,
    toolNamePreview: props.message.toolNamePreview,
    responseTextDraft: props.message.responseTextDraft,
    content: props.message.content ?? ''
  })
)

const footerCopyText = computed(() => props.copyText?.trim() || props.message.content?.trim() || '')

watch(rawContentViewEnabled, on => {
  if (!on) showRawWire.value = false
})

async function openScreenPreview() {
  screenLoading.value = true
  screenError.value = null
  screenPreview.value = null
  modalOpen.value = true
  try {
    const rel = screenRelPath.value
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
</script>

<template>
  <div class="tool-message-segment">
    <div
      v-if="toolCalls.length"
      class="px-3"
      :class="compactTop ? 'tool-block-shell-compact' : 'tool-block-shell'"
    >
      <ToolCallList
        :tool-calls="toolCalls"
        :show-tool-call-results="messageUi.showToolCallResults"
      />
    </div>

    <MessageFooterActions
      v-if="showFooter"
      class="px-3 !mt-0"
      :created-at="message.createdAt"
      :copy-text="footerCopyText"
      :show-copy="showCopyButton"
    >
      <template v-if="showDebugActions" #extra>
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
          type="button"
          class="message-action-btn"
          :class="showRawWire ? 'text-accent' : 'text-muted hover:text-foreground'"
          :title="showRawWire ? '隐藏原始内容' : '查看原始内容'"
          @click="showRawWire = !showRawWire"
        >
          <Code class="w-3.5 h-3.5" />
        </button>
      </template>
    </MessageFooterActions>

    <RawWirePanel
      v-if="showRawWire && (rawWireContent || rawWireReasoning || toolRawArgs)"
      :reasoning="rawWireReasoning"
      :raw-content="rawWireContent"
      :tool-raw-args="toolRawArgs"
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
