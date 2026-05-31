<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { ArrowDown } from 'lucide-vue-next'
import MessageRow from './message/MessageRow.vue'
import ToolMessageSegment from './message/assistant/ToolMessageSegment.vue'
import TaskBoardPanel from './TaskBoardPanel.vue'
import { useChatStore } from '../../stores/chat'
import { useSettingsStore } from '../../stores/settings'
import { uiForMessageAgent, useAgentsCatalog } from '../../composables/useAgentUi'
import { visibleToolCalls } from '../../lib/messageTooling'
import type { ChatMessage, TaskBoardDocument, ToolCall } from '../../types/chat'
import { assistantDisplayKind, isToolOnlyAssistantMessage } from '../../lib/assistantMessageKind'

const chat = useChatStore()
const settings = useSettingsStore()
const agentsCatalog = useAgentsCatalog()
const scroller = ref<HTMLDivElement | null>(null)
const showScrollButton = ref(false)

async function toBottom() {
  await nextTick()
  const el = scroller.value
  if (el) el.scrollTop = el.scrollHeight
}

function isNearBottom(): boolean {
  const el = scroller.value
  if (!el) return true
  return el.scrollHeight - el.scrollTop - el.clientHeight < 100
}

function onScroll() {
  showScrollButton.value = !isNearBottom()
}

onMounted(toBottom)

watch(() => chat.current?.messages.length, () => {
  if (isNearBottom()) toBottom()
})
watch(
  () => chat.current?.messages.map(m => m.content + (m.toolCalls?.length || 0)).join('|'),
  () => {
    if (isNearBottom()) toBottom()
  }
)

type ToolRunGroup = { id: string; toolCalls: ToolCall[]; message: ChatMessage }

type FlatEntry =
  | { type: 'message'; message: ChatMessage; trailingToolGroups?: ToolRunGroup[] }
  | { type: 'tool_run'; groups: ToolRunGroup[] }
  | { type: 'task_board'; anchorMessageId: string; storeKey: string; document: TaskBoardDocument; isActive: boolean }

function canAttachTrailingTools(message: ChatMessage): boolean {
  return (
    message.role === 'assistant'
    && !isToolOnlyAssistantMessage(message)
    && assistantDisplayKind(message) === 'model'
  )
}

function assistantMessageHadTools(entry: FlatEntry): boolean {
  if (entry.type !== 'message' || entry.message.role !== 'assistant') return false
  return (entry.message.toolCalls?.length ?? 0) > 0 || (entry.trailingToolGroups?.length ?? 0) > 0
}

function isTaskBoardTerminal(status: string | undefined): boolean {
  const s = (status ?? '').trim()
  return s === 'completed' || s === 'failed'
}

function visibleToolsForMessage(message: ChatMessage, toolCalls: ToolCall[]): ToolCall[] {
  const ui = uiForMessageAgent(message.agentId, message.agentName, settings.settings, agentsCatalog.value)
  if (!ui.showToolCalls) return []
  return visibleToolCalls(
    toolCalls,
    ui.hideToolNames,
    ui.showSidecarToolCalls === true,
    ui.showNonSidecarToolCalls !== false
  )
}

const flatMessages = computed<FlatEntry[]>(() => {
  const msgs = chat.current?.messages ?? []
  const entries: FlatEntry[] = []
  const convId = chat.currentId
  let toolGroups: ToolRunGroup[] = []

  function flushToolRun() {
    if (toolGroups.length === 0) return
    const last = entries[entries.length - 1]
    if (last?.type === 'message' && canAttachTrailingTools(last.message)) {
      last.trailingToolGroups = [...(last.trailingToolGroups ?? []), ...toolGroups]
    } else {
      entries.push({ type: 'tool_run', groups: [...toolGroups] })
    }
    toolGroups = []
  }

  for (const message of msgs) {
    if (isToolOnlyAssistantMessage(message)) {
      toolGroups.push({
        id: message.id,
        toolCalls: message.toolCalls ?? [],
        message
      })
    } else {
      flushToolRun()
      entries.push({ type: 'message', message })
    }

    const boards = chat.parentBoardsBoundToMessage(convId, message.id)
    for (const board of boards) {
      flushToolRun()
      entries.push({
        type: 'task_board',
        anchorMessageId: message.id,
        storeKey: board.storeKey,
        document: board.document,
        isActive: board.isActive
      })
    }
  }
  flushToolRun()
  return entries
})

function entrySpacing(entry: FlatEntry, index: number, entries: FlatEntry[]): string {
  if (index === 0) return ''

  const prev = entries[index - 1]
  const prevIsUser = prev.type === 'message' && prev.message.role === 'user'
  const prevIsAssistantText =
    prev.type === 'message'
    && prev.message.role === 'assistant'
    && !isToolOnlyAssistantMessage(prev.message)
  const prevIsToolRun = prev.type === 'tool_run'

  if (entry.type === 'tool_run') {
    if (prevIsAssistantText || prevIsUser) return 'mt-1.5'
    if (prevIsToolRun) return 'mt-0'
    return 'mt-1.5'
  }

  if (entry.type === 'message') {
    if (entry.message.role === 'user') return 'mt-7'
    if (prevIsToolRun) return 'mt-4'
    if (prevIsUser) return 'mt-7'
    if (prev.type === 'message' && prev.message.role === 'assistant') {
      return assistantMessageHadTools(prev) ? 'mt-4' : 'mt-7'
    }
    return 'mt-7'
  }

  return 'mt-4'
}
</script>

<template>
  <div ref="scroller" class="chat-scroll-area h-full overflow-y-auto chat-shell pb-6" @scroll="onScroll">
    <div class="chat-column pt-6 pb-10">
      <template
        v-for="(entry, index) in flatMessages"
        :key="entry.type === 'message'
          ? entry.message.id
          : entry.type === 'tool_run'
            ? `tool-run-${entry.groups.map(g => g.id).join('-')}`
            : `task-board-${entry.storeKey}`"
      >
        <div
          v-if="entry.type === 'message'"
          :class="entrySpacing(entry, index, flatMessages)"
        >
          <MessageRow
            :message="entry.message"
            :trailing-tool-groups="entry.trailingToolGroups"
          />
        </div>
        <div
          v-else-if="entry.type === 'tool_run'"
          class="tool-segments chat-column"
          :class="entrySpacing(entry, index, flatMessages)"
        >
          <ToolMessageSegment
            v-for="(group, gi) in entry.groups"
            :key="group.id"
            :message="group.message"
            :tool-calls="visibleToolsForMessage(group.message, group.toolCalls)"
            :message-ui="uiForMessageAgent(group.message.agentId, group.message.agentName, settings.settings, agentsCatalog)"
            :compact-top="gi > 0"
          />
        </div>
        <div
          v-else
          class="task-board-sticky mb-1 flex justify-end py-1"
          :class="[entrySpacing(entry, index, flatMessages), isTaskBoardTerminal(entry.document.meta?.status) ? '' : 'sticky top-0 z-20 bg-background/95 backdrop-blur-sm']"
        >
          <TaskBoardPanel
            :document="entry.document"
            :is-active="entry.isActive"
            :child-boards="chat.childBoardsForParent(chat.currentId, entry.storeKey)"
          />
        </div>
      </template>
    </div>

    <button
      v-if="showScrollButton"
      class="fixed bottom-32 right-8 h-10 w-10 rounded-full panel shadow-lg flex items-center justify-center cursor-pointer hover:bg-hover transition"
      @click="toBottom"
      title="滚动到底部"
    >
      <ArrowDown class="w-5 h-5 text-foreground" />
    </button>
  </div>
</template>

