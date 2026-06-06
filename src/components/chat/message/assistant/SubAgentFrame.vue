<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ChevronDown, ChevronRight, Code } from 'lucide-vue-next'
import type { AgentTrace, TaskBoardDocument } from '../../../../types/chat'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import { formatSubAgentSummaryLine } from '../../../../lib/subAgentStats'
import { traceAgentLabel } from '../../../../lib/agentUi'
import {
  subTraceHasVisibleActivity,
  isSubTraceUiCollapsed,
  ensureSubTraceSession
} from '../../../../lib/subAgentSession'
import { subAgentStatusLabel } from '../../../../lib/subAgentStats'
import { useSettingsStore } from '../../../../stores/settings'
import { useAgentsCatalog } from '../../../../composables/useAgentUi'
import AgentMessageBody, { type AgentMessageBodyModel } from './AgentMessageBody.vue'
import RawWirePanel from './RawWirePanel.vue'
import TaskBoardPanel from '../../TaskBoardPanel.vue'
import { hasTaskBoardContent } from '../../../../lib/taskBoard'
import { toolCallBaseName } from '../../../../lib/messageTooling'

const props = defineProps<{
  trace: AgentTrace
  messageUi: ResolvedAgentUi
  createdAt: number
  thoughtsDebugEnabled?: boolean
  generating: boolean
  isActiveGenerationMessage: boolean
  showMessageActions?: boolean
  childTaskBoardDocument?: TaskBoardDocument | null
}>()

const childBoard = computed(() =>
  props.childTaskBoardDocument && hasTaskBoardContent(props.childTaskBoardDocument)
    ? props.childTaskBoardDocument
    : null
)

const childBoardActive = computed(() => {
  const status = (childBoard.value?.meta?.status ?? '').trim()
  return status !== 'completed' && status !== 'failed'
})

const settingsStore = useSettingsStore()
const agentsCatalog = useAgentsCatalog()
const traceLabel = computed(() =>
  traceAgentLabel(props.trace, agentsCatalog.value, settingsStore.settings)
)
const rawContentViewEnabled = computed(() => settingsStore.settings.rawContentViewEnabled === true)

const session = computed(() => props.trace.session)

const isRunning = computed(() => props.trace.status === 'running')

const collapsed = computed(() => isSubTraceUiCollapsed(props.trace))

const summaryLine = computed(() => {
  if (isRunning.value && !subTraceHasVisibleActivity(props.trace)) {
    return `${traceLabel.value} · ${subAgentStatusLabel(props.trace.status)}…`
  }
  const s = session.value
  const persisted = s?.summaryLine?.trim()
  if (persisted && !isRunning.value) return persisted
  return formatSubAgentSummaryLine(
    traceLabel.value,
    props.trace.status,
    s?.stats ?? { searchCount: 0, readCount: 0, mouseCount: 0, inputCount: 0, otherCount: 0 }
  )
})

const bodyModel = computed((): AgentMessageBodyModel => {
  const s = session.value
  return {
    thoughts: s?.thoughts,
    toolNamePreview: s?.toolNamePreview,
    responseTextDraft: s?.responseTextDraft,
    reasoning: s?.reasoning,
    rawContent: s?.rawContent,
    content: undefined,
    contentStreaming: s?.contentStreaming === true,
    toolCalls: s?.toolCalls,
    status: props.trace.status === 'failed' ? 'error' : isRunning.value ? 'streaming' : 'done',
    createdAt: props.createdAt,
    errorMessage: props.trace.status === 'failed' ? props.trace.detail : undefined
  }
})

const subFrameActive = computed(
  () => props.generating && props.isActiveGenerationMessage && isRunning.value
)

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

function buildSessionToolRawArgs() {
  const calls = session.value?.toolCalls
  if (!calls?.length) return ''
  return calls
    .filter(tc => toolCallBaseName(tc.name) !== 'response')
    .map(tc => {
      const args = formatToolArgs(tc.arguments)
      return `[tool:${tc.name} id:${tc.id}]\n${args}`
    })
    .join('\n\n')
}

const toolRawArgs = computed(() => buildSessionToolRawArgs())

const hasRawWire = computed(() => {
  if (!rawContentViewEnabled.value) return false
  const s = session.value
  const reasoning = s?.reasoning?.trim() ?? ''
  const raw = s?.rawContent?.trim() ?? ''
  return reasoning.length > 0 || raw.length > 0 || toolRawArgs.value.trim().length > 0
})

const showRawWire = ref(false)

watch(rawContentViewEnabled, on => {
  if (!on) showRawWire.value = false
})

function toggleExpanded() {
  const s = ensureSubTraceSession(props.trace)
  if (isSubTraceUiCollapsed(props.trace)) {
    s.userExpanded = true
    s.collapsed = false
  } else {
    s.userExpanded = false
    s.collapsed = true
  }
}
</script>

<template>
  <div
    class="rounded-xl border-2 border-accent/25 bg-accent-muted/10 my-2 overflow-hidden"
    :class="collapsed ? 'py-2 px-3' : 'p-3'"
  >
    <button
      v-if="collapsed"
      type="button"
      class="w-full flex items-center gap-2 min-w-0 text-left hover:bg-hover/50 rounded-md px-1 py-0.5 transition"
      :aria-expanded="false"
      @click="toggleExpanded"
    >
      <ChevronRight class="w-4 h-4 shrink-0 text-accent" />
      <span class="flex-1 min-w-0 text-[13px] text-foreground truncate">{{ summaryLine }}</span>
    </button>

    <div v-else class="space-y-2">
      <div class="flex items-center gap-2 min-w-0">
        <button
          type="button"
          class="min-w-0 flex-1 flex items-center gap-2 text-left text-[13px] font-medium text-muted hover:text-foreground transition"
          :aria-expanded="true"
          @click="toggleExpanded"
        >
          <ChevronDown class="w-4 h-4 shrink-0 text-accent" />
          <span class="truncate">{{ traceLabel }}</span>
          <span class="text-xs text-muted shrink-0">{{ trace.status }}</span>
        </button>
        <button
          v-if="hasRawWire"
          type="button"
          class="message-action-btn shrink-0"
          :class="showRawWire ? 'text-accent' : 'text-muted hover:text-foreground'"
          :title="showRawWire ? '隐藏原始内容' : '查看原始内容'"
          @click.stop="showRawWire = !showRawWire"
        >
          <Code class="w-3.5 h-3.5" />
        </button>
      </div>

      <AgentMessageBody
        :body="bodyModel"
        :message-ui="messageUi"
        hide-response
        hide-copy
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :generating="generating"
        :is-active-generation-message="subFrameActive"
      />

      <TaskBoardPanel
        v-if="childBoard"
        class="mt-2"
        :document="childBoard"
        :is-active="childBoardActive"
      />

      <RawWirePanel
        v-if="showRawWire && hasRawWire"
        :reasoning="session?.reasoning"
        :raw-content="session?.rawContent"
        :tool-raw-args="toolRawArgs"
        @close="showRawWire = false"
      />
    </div>
  </div>
</template>
