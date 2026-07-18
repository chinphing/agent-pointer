<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, ref, watch } from 'vue'
import { ArrowDown } from 'lucide-vue-next'
import MessageRow from './message/MessageRow.vue'
import ToolMessageSegment from './message/assistant/ToolMessageSegment.vue'
import ToolRunGlueRow from './message/ToolRunGlueRow.vue'
import TaskBoardPanel from './TaskBoardPanel.vue'
import { useChatStore } from '../../stores/chat'
import { useSettingsStore } from '../../stores/settings'
import { uiForMessageAgent, useAgentsCatalog } from '../../composables/useAgentUi'
import { visibleToolCalls } from '../../lib/messageTooling'
import type { ChatMessage, TaskBoardDocument, ToolCall } from '../../types/chat'
import {
  assistantDisplayKind,
  isEphemeralDesktopNoticeMessage,
  isToolOnlyAssistantMessage
} from '../../lib/assistantMessageKind'
import { isToolRunContinuityGlue, shouldShowGlueMessage } from '../../lib/threadLayoutGlue'
import { isScopedSubMessage } from '../../lib/subAgentMessages'

const chat = useChatStore()
const settings = useSettingsStore()
const agentsCatalog = useAgentsCatalog()
const scroller = ref<HTMLDivElement | null>(null)
const showScrollButton = ref(false)

// ── Virtual rendering: only render a window of recent entries ──
const RENDER_WINDOW_INITIAL = 200
const RENDER_WINDOW_CHUNK = 100
const maxRender = ref(RENDER_WINDOW_INITIAL)

const hasMoreAbove = computed(() => flatMessages.value.length > maxRender.value)

/** Entries actually rendered in DOM — last `maxRender` entries of flatMessages. */
const renderedEntries = computed<FlatEntry[]>(() => {
  const all = flatMessages.value
  if (all.length <= maxRender.value) return all
  return all.slice(all.length - maxRender.value)
})

/** Scroll-to-top sentinel ref for load-more trigger. */
const topSentinel = ref<HTMLDivElement | null>(null)
let sentinelObserver: IntersectionObserver | null = null

function toBottom() {
  void nextTick(() => {
    const el = scroller.value
    if (el) el.scrollTop = el.scrollHeight
  })
}

function loadMoreAbove() {
  if (!hasMoreAbove.value) return
  const wasAtBottom = isNearBottom()
  // Capture current first visible entry to anchor scroll position after expansion.
  const el = scroller.value
  const savedHeight = el?.scrollHeight ?? 0
  maxRender.value += RENDER_WINDOW_CHUNK
  void nextTick(() => {
    if (el) {
      // Maintain scroll position relative to content bottom so visible area doesn't jump.
      const newHeight = el.scrollHeight
      el.scrollTop = newHeight - savedHeight + el.scrollTop
    }
    if (wasAtBottom) toBottom()
  })
}

function setupSentinel() {
  if (!topSentinel.value) return
  sentinelObserver = new IntersectionObserver(
    (entries) => {
      if (entries[0]?.isIntersecting && hasMoreAbove.value) {
        loadMoreAbove()
      }
    },
    { root: scroller.value, threshold: 0.1 }
  )
  sentinelObserver.observe(topSentinel.value)
}

function teardownSentinel() {
  sentinelObserver?.disconnect()
  sentinelObserver = null
}

onMounted(() => {
  toBottom()
  void nextTick(setupSentinel)
})

onBeforeUnmount(() => {
  teardownSentinel()
})

// Reset render window when conversation changes.
watch(() => chat.currentId, () => {
  maxRender.value = RENDER_WINDOW_INITIAL
  teardownSentinel()
  void nextTick(setupSentinel)
})

// Auto-expand window to include newly arriving messages.
watch(() => chat.current?.messages.length, (len) => {
  if (len !== undefined && len > 0 && maxRender.value < len) {
    // New messages arrived — ensure they're within the render window.
    maxRender.value = Math.max(maxRender.value, len)
  }
  if (isNearBottom()) toBottom()
})
watch(
  () => chat.current?.messages.map(m => m.content + (m.toolCalls?.length || 0)).join('|'),
  () => {
    if (isNearBottom()) toBottom()
  }
)

function isNearBottom(): boolean {
  const el = scroller.value
  if (!el) return true
  return el.scrollHeight - el.scrollTop - el.clientHeight < 100
}

function onScroll() {
  showScrollButton.value = !isNearBottom()
}

type ToolRunGroup = { id: string; toolCalls: ToolCall[]; message: ChatMessage }

type ToolRunItem =
  | { kind: 'tools'; group: ToolRunGroup }
  | { kind: 'glue'; message: ChatMessage }

type FlatEntry =
  | { type: 'message'; message: ChatMessage; trailingToolGroups?: ToolRunGroup[]; compact?: boolean }
  | { type: 'tool_run'; items: ToolRunItem[] }
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

function toolRunHasTools(items: ToolRunItem[]): boolean {
  return items.some(i => i.kind === 'tools')
}

function shouldShowThreadGlue(message: ChatMessage): boolean {
  return shouldShowGlueMessage(message, settings.settings)
}

const flatMessages = computed<FlatEntry[]>(() => {
  void chat.taskBoards
  const msgs = chat.current?.messages ?? []
  const entries: FlatEntry[] = []
  const convId = chat.currentId
  let toolRunItems: ToolRunItem[] = []

  function flushToolRun() {
    if (toolRunItems.length === 0) return
    if (!toolRunHasTools(toolRunItems)) {
      for (const item of toolRunItems) {
        if (item.kind === 'glue' && shouldShowThreadGlue(item.message)) {
          entries.push({ type: 'message', message: item.message, compact: true })
        }
      }
      toolRunItems = []
      return
    }
    const last = entries[entries.length - 1]
    const groups = toolRunItems
      .filter((i): i is { kind: 'tools'; group: ToolRunGroup } => i.kind === 'tools')
      .map(i => i.group)
    const hasGlue = toolRunItems.some(
      i => i.kind === 'glue' && shouldShowThreadGlue(i.message)
    )
    if (last?.type === 'message' && canAttachTrailingTools(last.message) && !hasGlue) {
      last.trailingToolGroups = [...(last.trailingToolGroups ?? []), ...groups]
    } else {
      entries.push({ type: 'tool_run', items: [...toolRunItems] })
    }
    toolRunItems = []
  }

  for (const message of msgs) {
    if (isScopedSubMessage(message)) continue
    if (isEphemeralDesktopNoticeMessage(message)) {
      flushToolRun()
      entries.push({ type: 'message', message })
    } else if (isToolOnlyAssistantMessage(message)) {
      const visible = visibleToolsForMessage(message, message.toolCalls ?? [])
      if (visible.length > 0) {
        toolRunItems.push({
          kind: 'tools',
          group: {
            id: message.id,
            toolCalls: message.toolCalls ?? [],
            message
          }
        })
      }
    } else if (isToolRunContinuityGlue(message)) {
      if (toolRunHasTools(toolRunItems)) {
        if (shouldShowThreadGlue(message)) {
          toolRunItems.push({ kind: 'glue', message })
        }
      } else if (shouldShowThreadGlue(message)) {
        flushToolRun()
        entries.push({ type: 'message', message, compact: true })
      }
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
    && !isEphemeralDesktopNoticeMessage(prev.message)
  const prevIsToolRun = prev.type === 'tool_run'
  const prevIsDesktopNotice =
    prev.type === 'message'
    && prev.message.role === 'assistant'
    && isEphemeralDesktopNoticeMessage(prev.message)
  const prevIsTaskBoard = prev.type === 'task_board'

  if (entry.type === 'tool_run') {
    if (prevIsTaskBoard || prevIsToolRun) return 'mt-0'
    if (prevIsDesktopNotice) return 'mt-0.5'
    if (prevIsAssistantText || prevIsUser) return 'mt-1.5'
    return 'mt-1.5'
  }

  if (entry.type === 'task_board') {
    if (prevIsToolRun || prevIsTaskBoard) return 'mt-0.5'
    if (prevIsDesktopNotice) return 'mt-0.5'
    return 'mt-1.5'
  }

  if (entry.type === 'message') {
    const isCompact = entry.compact === true
    const isDesktopNotice =
      entry.message.role === 'assistant' && isEphemeralDesktopNoticeMessage(entry.message)
    if (isDesktopNotice) {
      if (prevIsToolRun || prevIsDesktopNotice) return 'mt-0.5'
      return 'mt-1.5'
    }
    if (isCompact) {
      if (prevIsToolRun) return 'mt-0'
      return 'mt-1'
    }
    if (entry.message.role === 'user') return 'mt-7'
    if (prevIsToolRun) return 'mt-4'
    if (prevIsDesktopNotice) return 'mt-1.5'
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
      <!-- Sentinel element: when this becomes visible, load more history above -->
      <div v-if="hasMoreAbove" ref="topSentinel" class="flex items-center justify-center py-3 text-xs text-muted-foreground cursor-pointer hover:text-foreground transition-colors" @click="loadMoreAbove">
        <span>加载更多历史消息…</span>
      </div>
      <template
        v-for="(entry, index) in renderedEntries"
        :key="entry.type === 'message'
          ? entry.message.id
          : entry.type === 'tool_run'
            ? `tool-run-${entry.items.map(i => i.kind === 'tools' ? i.group.id : i.message.id).join('-')}`
            : `task-board-${entry.storeKey}-${entry.anchorMessageId}`"
      >
        <div
          v-if="entry.type === 'message'"
          :class="entrySpacing(entry, index, renderedEntries)"
        >
          <ToolRunGlueRow v-if="entry.compact && shouldShowThreadGlue(entry.message)" :message="entry.message" />
          <MessageRow
            v-else
            :message="entry.message"
            :trailing-tool-groups="entry.trailingToolGroups"
          />
        </div>
        <div
          v-else-if="entry.type === 'tool_run'"
          class="tool-segments chat-column tool-only-message"
          :class="entrySpacing(entry, index, renderedEntries)"
        >
          <template v-for="item in entry.items" :key="item.kind === 'tools' ? item.group.id : item.message.id">
            <ToolRunGlueRow
              v-if="item.kind === 'glue' && shouldShowThreadGlue(item.message)"
              :message="item.message"
            />
            <ToolMessageSegment
              v-else-if="item.kind === 'tools'"
              :message="item.group.message"
              :tool-calls="visibleToolsForMessage(item.group.message, item.group.toolCalls)"
              :message-ui="uiForMessageAgent(item.group.message.agentId, item.group.message.agentName, settings.settings, agentsCatalog)"
              compact-top
              hide-footer
            />
          </template>
        </div>
        <div
          v-else
          class="task-board-sticky mb-1 flex justify-end py-1"
          :class="[entrySpacing(entry, index, renderedEntries), isTaskBoardTerminal(entry.document.meta?.status) ? '' : 'sticky top-0 z-20 bg-background/95 backdrop-blur-sm']"
        >
          <TaskBoardPanel
            :document="entry.document"
            :is-active="entry.isActive"
            :child-boards="chat.childBoardsForParent(chat.currentId, entry.storeKey)"
            :conversation-id="chat.currentId"
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

