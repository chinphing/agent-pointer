import { computed, ref, watch, type Ref } from 'vue'
import { storeToRefs } from 'pinia'
import type { ChatMessage, ComputerAnnotatedPreview } from '../types/chat'
import { useSettingsStore } from '../stores/settings'
import { useChatStore } from '../stores/chat'
import { previewComputerAnnotatedScreen, previewComputerRoundScreen } from '../lib/api'
import { isMessageStreaming } from '../lib/assistantMessageKind'
import { showAnnotatedScreenAction } from '../lib/computerMessageContext'
import { toolCallBaseName } from '../lib/messageTooling'

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

export function useAssistantMessageDebug(
  message: Ref<ChatMessage | undefined>,
  options: {
    generating: Ref<boolean>
    isActiveGenerationMessage: Ref<boolean>
  }
) {
  const settingsStore = useSettingsStore()
  const chatStore = useChatStore()
  const { generating, activeGeneratingMessageId } = storeToRefs(chatStore)

  const showRawWire = ref(false)
  const modalOpen = ref(false)
  const screenLoading = ref(false)
  const screenPreview = ref<ComputerAnnotatedPreview | null>(null)
  const screenError = ref<string | null>(null)

  const rawContentViewEnabled = computed(() => settingsStore.settings.rawContentViewEnabled === true)

  const toolRawArgs = computed(() => {
    const calls = message.value?.toolCalls
    if (!calls?.length) return ''
    return calls
      .filter(tc => toolCallBaseName(tc.name) !== 'response')
      .map(tc => {
        const args = formatToolArgs(tc.arguments)
        return `[tool:${tc.name} id:${tc.id}]\n${args}`
      })
      .join('\n\n')
  })

  const screenRelPath = computed(() => message.value?.computerRoundScreenRelPath?.trim() ?? '')

  const rawWireReasoning = computed(() => message.value?.reasoning?.trim() || undefined)

  const rawWireContent = computed(() => {
    const msg = message.value
    if (!msg) return undefined
    const raw = msg.rawContent?.trim() ?? ''
    if (raw && raw !== (msg.content?.trim() ?? '')) return raw
    return undefined
  })

  const hasRawWire = computed(() => {
    if (!rawContentViewEnabled.value) return false
    return !!(rawWireContent.value?.trim() || rawWireReasoning.value?.trim() || toolRawArgs.value.trim())
  })

  /** Raw content button/panel visibility independent of streaming/generation state.
   *  Users must be able to inspect raw LLM output during active execution. */
  const showRawWireFooter = computed(() => hasRawWire.value)

  const showCamera = computed(() => {
    const msg = message.value
    if (!msg) return false
    if (settingsStore.settings.computerAnnotatedScreenViewEnabled !== true) return false
    return showAnnotatedScreenAction(msg, {
      agentMode: settingsStore.settings.agentMode,
      leadAgentId: settingsStore.settings.leadAgentId ?? '',
      annotatedScreenViewEnabled: true
    })
  })

  const isStreaming = computed(() => isMessageStreaming(message.value?.status ?? 'done'))

  const isActiveForMessage = computed(
    () => message.value?.id === activeGeneratingMessageId.value
  )

  const showMessageActions = computed(() => {
    if (generating.value && isActiveForMessage.value) return false
    if (options.generating.value && options.isActiveGenerationMessage.value) return false
    if (message.value?.status === 'pending') return false
    return !isStreaming.value
  })

  const showDebugActions = computed(
    () => showMessageActions.value && showCamera.value
  )

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

  return {
    showRawWire,
    modalOpen,
    screenLoading,
    screenPreview,
    screenError,
    rawWireReasoning,
    rawWireContent,
    toolRawArgs,
    hasRawWire,
    showCamera,
    showMessageActions,
    showDebugActions,
    showRawWireFooter,
    openScreenPreview
  }
}
