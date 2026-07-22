<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, provide, ref, watch } from 'vue'
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
import {
  PAGE_SEARCH_MARK_CLASS,
  clearSearchTextMarks,
  clearSidebarSearchTextMarks,
  highlightSearchText,
  highlightSidebarSearchText
} from '../../lib/sidebarSearchTextHighlight'

const props = withDefaults(defineProps<{
  searchMatchIds?: string[]
  searchMatchToolCallIds?: string[]
  activeSearchMessageId?: string | null
  activeSearchToolCallId?: string | null
  searchQuery?: string
}>(), {
  searchMatchIds: () => [],
  searchMatchToolCallIds: () => [],
  activeSearchMessageId: null,
  activeSearchToolCallId: null,
  searchQuery: ''
})

const chat = useChatStore()
const settings = useSettingsStore()
const agentsCatalog = useAgentsCatalog()
const scroller = ref<HTMLDivElement | null>(null)
const showScrollButton = ref(false)
/** Temporary highlight after sidebar search locate. */
const focusHighlightMessageId = ref<string | null>(null)
let focusHighlightTimer: ReturnType<typeof setTimeout> | null = null
let focusMarkedRoot: HTMLElement | null = null
/** While locating a search hit, do not auto-scroll to bottom. */
const locatingFocus = ref(false)

provide('currentConversationSearchToolCallIds', computed(() => props.searchMatchToolCallIds))
provide('currentConversationActiveToolCallId', computed(() => props.activeSearchToolCallId))

// ── Virtual rendering: only render a window of recent entries ──
const RENDER_WINDOW_INITIAL = 50
const RENDER_WINDOW_CHUNK = 25
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
  if (locatingFocus.value) return
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
  if (focusHighlightTimer != null) {
    clearTimeout(focusHighlightTimer)
    focusHighlightTimer = null
  }
  if (focusMarkedRoot) {
    clearSidebarSearchTextMarks(focusMarkedRoot)
    focusMarkedRoot = null
  }
  if (scroller.value) clearSearchTextMarks(scroller.value, PAGE_SEARCH_MARK_CLASS)
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
  if (!locatingFocus.value && isNearBottom()) toBottom()
})
watch(
  () => chat.current?.messages.map(m => m.content + (m.toolCalls?.length || 0)).join('|'),
  () => {
    if (!locatingFocus.value && isNearBottom()) toBottom()
  }
)

function entryContainsMessageId(entry: FlatEntry, messageId: string): boolean {
  const id = messageId.trim()
  if (!id) return false
  if (entry.type === 'message') return entry.message.id === id
  if (entry.type === 'tool_run') {
    return entry.items.some(item =>
      item.kind === 'tools'
        ? item.group.message.id === id || item.group.id === id
        : item.message.id === id
    )
  }
  return entry.anchorMessageId === id
}

function entryPrimaryMessageId(entry: FlatEntry): string | undefined {
  if (entry.type === 'message') return entry.message.id
  if (entry.type === 'tool_run') {
    for (const item of entry.items) {
      if (item.kind === 'tools') return item.group.message.id
    }
    for (const item of entry.items) {
      if (item.kind === 'glue') return item.message.id
    }
    return undefined
  }
  return entry.anchorMessageId
}

function entryIsFocusHighlight(entry: FlatEntry): boolean {
  const focus = focusHighlightMessageId.value
  if (!focus) return false
  return entryContainsMessageId(entry, focus)
}

function messageIdIsSearchMatch(messageId: string): boolean {
  return props.searchMatchIds.includes(messageId)
}

function messageIdIsActiveSearchMatch(messageId: string): boolean {
  return !props.activeSearchToolCallId && props.activeSearchMessageId?.trim() === messageId
}

function messageIdIsFocusHighlight(messageId: string): boolean {
  return focusHighlightMessageId.value === messageId
}

function entryIsSearchMatch(entry: FlatEntry): boolean {
  return props.searchMatchIds.some(id => entryContainsMessageId(entry, id))
}

function entryIsActiveSearchMatch(entry: FlatEntry): boolean {
  if (props.activeSearchToolCallId) return false
  const id = props.activeSearchMessageId?.trim()
  return !!id && entryContainsMessageId(entry, id)
}

function expandRenderWindowForMessage(messageId: string): boolean {
  const all = flatMessages.value
  const idx = all.findIndex(e => entryContainsMessageId(e, messageId))
  if (idx < 0) return false
  // Window is the last `maxRender` entries — grow it so `idx` is included.
  const need = all.length - idx
  if (maxRender.value < need) {
    maxRender.value = need
  }
  return true
}

function clearFocusTextMarks() {
  if (!focusMarkedRoot) return
  clearSidebarSearchTextMarks(focusMarkedRoot)
  focusMarkedRoot = null
}

function clearFocusHighlightSoon(messageId: string) {
  if (focusHighlightTimer != null) clearTimeout(focusHighlightTimer)
  focusHighlightTimer = setTimeout(() => {
    if (focusHighlightMessageId.value === messageId) {
      focusHighlightMessageId.value = null
    }
    clearFocusTextMarks()
    focusHighlightTimer = null
  }, 2400)
}

async function tryLocatePendingFocus() {
  const pending = chat.pendingFocusMessage
  if (!pending) return
  if (chat.currentId !== pending.conversationId) return

  const msgs = chat.current?.messages ?? []
  if (msgs.length === 0) return

  const targetId = pending.messageId.trim()
  if (!targetId) {
    chat.clearPendingFocusMessage()
    return
  }

  const inMessages = msgs.some(m => m.id === targetId)
  if (!inMessages) {
    if (chat.isCurrentConversationHydrating) return
    console.warn('[MessageList] focus message not found after hydrate', targetId)
    chat.clearPendingFocusMessage()
    return
  }

  locatingFocus.value = true
  try {
    if (!expandRenderWindowForMessage(targetId)) {
      maxRender.value = Math.max(maxRender.value, flatMessages.value.length)
    }
    await nextTick()
    const root = scroller.value
    if (!root) return
    const el = root.querySelector(
      `[data-message-id="${CSS.escape(targetId)}"]`
    ) as HTMLElement | null
    if (!el) {
      console.warn('[MessageList] focus message DOM node missing', targetId)
      chat.clearPendingFocusMessage()
      return
    }
    clearFocusTextMarks()
    // Apply the reactive message outline first. Its Vue patch may rewrite v-html content,
    // so precise text marks must be inserted only after that patch has settled.
    focusHighlightMessageId.value = targetId
    await nextTick()
    const settledEl = root.querySelector(
      `[data-message-id="${CSS.escape(targetId)}"]`
    ) as HTMLElement | null
    if (!settledEl) {
      console.warn('[MessageList] focus message DOM node missing after highlight patch', targetId)
      chat.clearPendingFocusMessage()
      return
    }
    const precise = pending.queryTerm
      ? highlightSidebarSearchText(settledEl, pending.queryTerm)
      : { count: 0, scrollTarget: null }
    if (precise.count > 0) focusMarkedRoot = settledEl
    const scrollTarget = precise.scrollTarget ?? settledEl
    scrollTarget.scrollIntoView({ block: 'center', behavior: 'smooth' })
    clearFocusHighlightSoon(targetId)
    chat.clearPendingFocusMessage()
    console.info('[MessageList] focused search hit message', targetId)
  } finally {
    // Keep suppressing bottom-scroll briefly so smooth scroll is not fought.
    window.setTimeout(() => {
      locatingFocus.value = false
    }, 600)
  }
}

watch(
  () =>
    [
      chat.pendingFocusMessage?.conversationId,
      chat.pendingFocusMessage?.messageId,
      chat.pendingFocusMessage?.queryTerm,
      chat.currentId,
      chat.current?.messages.length,
      chat.isCurrentConversationHydrating
    ] as const,
  () => {
    void tryLocatePendingFocus()
  }
)

watch(
  () => [
    props.activeSearchMessageId,
    props.activeSearchToolCallId,
    props.searchQuery
  ] as const,
  async ([messageId, toolCallId, searchQuery]) => {
    const root = scroller.value
    if (root) clearSearchTextMarks(root, PAGE_SEARCH_MARK_CLASS)

    const targetMessageId = messageId?.trim()
    const query = searchQuery.trim()
    if (!targetMessageId || !query) return
    locatingFocus.value = true
    try {
      if (!expandRenderWindowForMessage(targetMessageId)) return
      await nextTick()
      const settledRoot = scroller.value
      if (!settledRoot) return
      const targetToolCallId = toolCallId?.trim()
      const selector = targetToolCallId
        ? `[data-tool-call-id="${CSS.escape(targetToolCallId)}"]`
        : `[data-message-id="${CSS.escape(targetMessageId)}"]`
      const el = settledRoot.querySelector(selector) as HTMLElement | null
      if (!el) return
      const precise = highlightSearchText(el, query, {
        markClass: PAGE_SEARCH_MARK_CLASS,
        excludeSelector: targetToolCallId ? undefined : '[data-tool-call-id]'
      })
      ;(precise.scrollTarget ?? el).scrollIntoView({ block: 'center', behavior: 'smooth' })
    } finally {
      window.setTimeout(() => {
        locatingFocus.value = false
      }, 600)
    }
  },
  { immediate: true }
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
          :data-message-id="entryPrimaryMessageId(entry)"
          :class="[
            entrySpacing(entry, index, renderedEntries),
            entryIsActiveSearchMatch(entry)
              ? 'rounded-xl ring-2 ring-accent/60 bg-accent/10 transition-colors'
              : entryIsFocusHighlight(entry)
                ? 'rounded-xl ring-2 ring-accent/60 bg-accent/10 transition-colors'
                : entryIsSearchMatch(entry)
                  ? 'rounded-xl bg-accent/5 transition-colors'
                  : ''
          ]"
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
            <div
              :data-message-id="item.kind === 'tools' ? item.group.message.id : item.message.id"
              :class="[
                messageIdIsActiveSearchMatch(item.kind === 'tools' ? item.group.message.id : item.message.id)
                  ? 'rounded-xl ring-2 ring-accent/60 bg-accent/10 transition-colors'
                  : messageIdIsFocusHighlight(item.kind === 'tools' ? item.group.message.id : item.message.id)
                    ? 'rounded-xl ring-2 ring-accent/60 bg-accent/10 transition-colors'
                    : messageIdIsSearchMatch(item.kind === 'tools' ? item.group.message.id : item.message.id)
                      ? 'rounded-xl bg-accent/5 transition-colors'
                      : ''
              ]"
            >
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
            </div>
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

