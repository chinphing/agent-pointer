<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ChevronDown, ChevronRight, Code } from 'lucide-vue-next'
import type { AgentTrace } from '../../../../types/chat'
import type { ResolvedAgentUi } from '../../../../lib/agentUi'
import { formatSubAgentSummaryLine } from '../../../../lib/subAgentStats'
import {
  runningSubTraceSummaryLine,
  subTraceHasVisibleActivity
} from '../../../../lib/subAgentSession'
import { useSettingsStore } from '../../../../stores/settings'
import AgentMessageBody, { type AgentMessageBodyModel } from './AgentMessageBody.vue'
import RawWirePanel from './RawWirePanel.vue'

const props = defineProps<{
  trace: AgentTrace
  messageUi: ResolvedAgentUi
  createdAt: number
  thoughtsDebugEnabled?: boolean
  generating: boolean
  isActiveGenerationMessage: boolean
  showMessageActions?: boolean
}>()

const settingsStore = useSettingsStore()
const rawContentViewEnabled = computed(() => settingsStore.settings.rawContentViewEnabled === true)

const session = computed(() => props.trace.session)

const isRunning = computed(() => props.trace.status === 'running')

const collapsed = computed(() => {
  const s = session.value
  if (!s) return false
  if (s.userExpanded) return false
  return s.collapsed
})

const summaryLine = computed(() => {
  if (isRunning.value && !subTraceHasVisibleActivity(props.trace)) {
    return runningSubTraceSummaryLine(props.trace)
  }
  const s = session.value
  if (s?.summaryLine?.trim()) return s.summaryLine.trim()
  return formatSubAgentSummaryLine(
    props.trace.name,
    props.trace.status,
    s?.stats ?? { searchCount: 0, readCount: 0 }
  )
})

const bodyModel = computed((): AgentMessageBodyModel => {
  const s = session.value
  return {
    thoughts: s?.thoughts,
    headline: s?.headline,
    toolNamePreview: s?.toolNamePreview,
    responseTextDraft: s?.responseTextDraft,
    reasoning: s?.reasoning,
    rawContent: s?.rawContent,
    content: undefined,
    contentStreaming: s?.contentStreaming ?? isRunning.value,
    toolCalls: s?.toolCalls,
    status: props.trace.status === 'failed' ? 'error' : isRunning.value ? 'streaming' : 'done',
    createdAt: props.createdAt,
    errorMessage: props.trace.status === 'failed' ? props.trace.detail : undefined
  }
})

const showStartupPlaceholder = computed(
  () => isRunning.value && !subTraceHasVisibleActivity(props.trace)
)

const subFrameActive = computed(
  () => props.generating && props.isActiveGenerationMessage && isRunning.value
)

const hasRawWire = computed(() => {
  if (!rawContentViewEnabled.value) return false
  const s = session.value
  const reasoning = s?.reasoning?.trim() ?? ''
  const raw = s?.rawContent?.trim() ?? ''
  return reasoning.length > 0 || raw.length > 0
})

const showRawWire = ref(false)

watch(rawContentViewEnabled, on => {
  if (!on) showRawWire.value = false
})

function toggleExpanded() {
  const s = session.value
  if (!s) return
  s.userExpanded = !s.userExpanded
  if (s.userExpanded) s.collapsed = false
  else if (props.trace.status === 'completed' || props.trace.status === 'failed') s.collapsed = true
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
          <span class="truncate">{{ trace.name }}</span>
          <span class="text-xs text-muted shrink-0">{{ trace.status }}</span>
        </button>
        <button
          v-if="hasRawWire && showMessageActions !== false"
          type="button"
          class="message-action-btn shrink-0"
          :class="showRawWire ? 'text-accent' : 'text-muted hover:text-foreground'"
          :title="showRawWire ? '隐藏原始内容' : '查看原始内容'"
          @click.stop="showRawWire = !showRawWire"
        >
          <Code class="w-3.5 h-3.5" />
        </button>
      </div>

      <div
        v-if="showStartupPlaceholder"
        class="flex items-center gap-2 text-sm text-muted px-1 py-2"
      >
        <span class="typing-dot" />
        <span class="typing-dot" style="animation-delay: 0.2s" />
        <span class="typing-dot" style="animation-delay: 0.4s" />
        <span class="text-[13px]">子 Agent 启动中…</span>
      </div>

      <AgentMessageBody
        v-else
        :body="bodyModel"
        :message-ui="messageUi"
        hide-response
        hide-copy
        :thoughts-debug-enabled="thoughtsDebugEnabled"
        :generating="generating"
        :is-active-generation-message="subFrameActive || isActiveGenerationMessage"
      />

      <RawWirePanel
        v-if="showRawWire && hasRawWire"
        :reasoning="session?.reasoning"
        :raw-content="session?.rawContent"
        @close="showRawWire = false"
      />
    </div>
  </div>
</template>
