<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { Code, Camera } from 'lucide-vue-next'
import type { ChatMessage, ComputerAnnotatedPreview } from '../../../../types/chat'
import { useSettingsStore } from '../../../../stores/settings'
import { useChatStore } from '../../../../stores/chat'
import { previewComputerAnnotatedScreen, previewComputerRoundScreen } from '../../../../lib/api'
import { isTauriRuntime } from '../../../../lib/runtime'
import { shouldShowSubAgentTrace, uiForSubAgentFrame } from '../../../../lib/agentUi'
import { useAgentsCatalog, uiForMessageAgent } from '../../../../composables/useAgentUi'
import { isMessageStreaming } from '../../../../lib/assistantMessageKind'
import { showAnnotatedScreenAction } from '../../../../lib/computerMessageContext'
import { subTracesForMessage } from '../../../../lib/subAgentSession'
import AgentMessageBody, { type AgentMessageBodyModel } from './AgentMessageBody.vue'
import SubAgentFrame from './SubAgentFrame.vue'
import ModelThoughtPanels from './ModelThoughtPanels.vue'
import RawWirePanel from './RawWirePanel.vue'
import ScreenPreviewModal from './ScreenPreviewModal.vue'

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

const subTraces = computed(() => subTracesForMessage(props.message))

function subTraceUi(trace: (typeof subTraces.value)[number]) {
  return uiForSubAgentFrame(
    trace,
    settingsStore.settings,
    agentsCatalog.value,
    messageUi.value
  )
}

const showRawWire = ref(false)
const modalOpen = ref(false)
const screenLoading = ref(false)
const screenPreview = ref<ComputerAnnotatedPreview | null>(null)
const screenError = ref<string | null>(null)

const thoughtsDebugEnabled = computed(() => messageUi.value.showThoughts)

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

const chatStore = useChatStore()
const { generating, activeGeneratingMessageId } = storeToRefs(chatStore)

const isActiveGenerationMessage = computed(
  () => props.message.id === activeGeneratingMessageId.value
)

const isStreaming = computed(() => isMessageStreaming(props.message.status))

const showMessageActions = computed(() => {
  if (generating.value && isActiveGenerationMessage.value) return false
  if (props.message.status === 'pending') return false
  return true
})

const showActionBar = computed(
  () => showCamera.value || (showMessageActions.value && hasRawWire.value)
)

const leadBody = computed((): AgentMessageBodyModel => ({
  thoughts: props.message.thoughts,
  headline: props.message.headline,
  toolNamePreview: props.message.toolNamePreview,
  responseTextDraft: props.message.responseTextDraft,
  reasoning: props.message.reasoning,
  content: props.message.content,
  rawContent: props.message.rawContent,
  contentStreaming: props.message.contentStreaming,
  toolCalls: props.message.toolCalls,
  status: props.message.status,
  createdAt: props.message.createdAt,
  errorMessage: props.message.errorMessage
}))

const showSupervisorPlan = computed(
  () =>
    showSubAgentTrace.value &&
    (props.message.supervisorPlanTasks?.length ?? 0) > 0 &&
    subTraces.value.length === 0
)

watch(rawContentViewEnabled, on => {
  if (!on) showRawWire.value = false
})

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
</script>

<template>
  <div class="w-full max-w-full space-y-2">
    <ModelThoughtPanels
      v-if="showSupervisorPlan"
      :plan-tasks="message.supervisorPlanTasks"
      :is-streaming="isStreaming"
    />

    <AgentMessageBody
      :body="leadBody"
      :message-ui="messageUi"
      :thoughts-debug-enabled="thoughtsDebugEnabled"
      :generating="generating"
      :is-active-generation-message="isActiveGenerationMessage"
    />

    <SubAgentFrame
      v-for="trace in subTraces"
      v-show="showSubAgentTrace"
      :key="trace.id"
      :trace="trace"
      :message-ui="subTraceUi(trace)"
      :created-at="message.createdAt"
      :thoughts-debug-enabled="thoughtsDebugEnabled"
      :generating="generating"
      :is-active-generation-message="isActiveGenerationMessage"
      :show-message-actions="showMessageActions"
    />

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
