<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, provide, ref, watch } from 'vue'
import type { ComponentPublicInstance } from 'vue'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { ArrowDown, ChevronDown, ChevronRight } from 'lucide-vue-next'
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
  assistantHasDeliverableContent,
  isEphemeralDesktopNoticeMessage,
  isToolOnlyAssistantMessage
} from '../../lib/assistantMessageKind'
import { isToolRunContinuityGlue, shouldShowGlueMessage } from '../../lib/threadLayoutGlue'
import { isScopedSubMessage } from '../../lib/subAgentMessages'
import { isCompressionSummaryMessage } from '../../lib/compressionMessage'
import { messageRowSpacingPixels, messageTurnSpacingPixels, messageVirtualizerBaseOptions } from '../../lib/messageVirtualization'
import { buildConversationTurns, shouldAutoExpandTurn, turnContains } from '../../lib/conversationTurns'
import { elapsedBetweenTimestamps, formatTurnElapsed, turnElapsedMs } from '../../lib/turnElapsed'
import { shouldStickActiveTaskBoard } from '../../lib/taskBoardSticky'
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
const activeBoardInlineScrollTop = ref<number | null>(null)
const activeBoardIsSticky = ref(false)
/** Temporary highlight after sidebar search locate. */
const focusHighlightMessageId = ref<string | null>(null)
let focusHighlightTimer: ReturnType<typeof setTimeout> | null = null
let focusMarkedRoot: HTMLElement | null = null
/** While locating a search hit, do not auto-scroll to bottom. */
const locatingFocus = ref(false)

provide('currentConversationSearchToolCallIds', computed(() => props.searchMatchToolCallIds))
provide('currentConversationActiveToolCallId', computed(() => props.activeSearchToolCallId))

// ── Bidirectional virtual rendering ──
// Rows are variable-height and measured after mount. Only visible rows plus overscan
// stay in the DOM, regardless of where the user or search target is in the thread.
let wasNearBottomBeforeUpdate = true
let scrollFrame: number | null = null

function scheduleToBottom() {
  if (scrollFrame != null) return
  scrollFrame = requestAnimationFrame(() => {
    scrollFrame = null
    toBottom()
  })
}

function toBottom() {
  if (locatingFocus.value || conversationTurns.value.length === 0) return
  void nextTick(() => {
    rowVirtualizer.value.scrollToIndex(conversationTurns.value.length - 1, {
      align: 'end',
      behavior: 'auto'
    })
  })
}

onMounted(() => {
  void nextTick(() => {
    toBottom()
    // Sidebar search focus can already be pending when this component is mounted
    // after the hydration skeleton was replaced. In that case the watcher below
    // registered too late to observe the state change, so retry from mounted.
    void tryLocatePendingFocus()
  })
})

onBeforeUnmount(() => {
  if (scrollFrame != null) {
    cancelAnimationFrame(scrollFrame)
    scrollFrame = null
  }
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

watch(() => chat.currentId, async () => {
  await nextTick()
  rowVirtualizer.value.measure()
  toBottom()
})

watch(() => chat.current?.messages.length, () => {
  const shouldFollow = wasNearBottomBeforeUpdate && !locatingFocus.value
  if (!shouldFollow) return

  // Keep prior row measurements when appending. Clearing the whole cache makes
  // older variable-height turns briefly fall back to estimates and shifts the viewport.
  void nextTick(() => scheduleToBottom())
})
const activeRenderSignal = computed(() => {
  const messages = chat.current?.messages ?? []
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index]!
    if (message.status !== 'streaming' && message.status !== 'pending') continue
    const toolSignal = message.toolCalls
      ?.map(tool => `${tool.id}:${tool.status}:${tool.result?.length ?? 0}`)
      .join(',') ?? ''
    return `${message.id}:${message.content.length}:${message.reasoning?.length ?? 0}:${toolSignal}`
  }
  return ''
})

watch(activeRenderSignal, () => {
  const shouldFollow = isNearBottom() && !locatingFocus.value
  if (shouldFollow) scheduleToBottom()
})

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

function nextAnimationFrame(): Promise<void> {
  return new Promise(resolve => requestAnimationFrame(() => resolve()))
}

async function waitForMessageElement(
  messageId: string,
  attempts = 5
): Promise<HTMLElement | null> {
  const root = scroller.value
  if (!root) return null
  const selector = `[data-message-id="${CSS.escape(messageId)}"]`
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const element = root.querySelector(selector) as HTMLElement | null
    if (element) return element
    await nextTick()
    await nextAnimationFrame()
  }
  return root.querySelector(selector) as HTMLElement | null
}

async function mountEntryForMessage(messageId: string): Promise<number | null> {
  const idx = expandTurnContainingMessage(messageId)
  if (idx < 0) return null

  // Expanding a collapsed Turn changes its height. Let Vue render the full entries,
  // then measure that row before scrolling so TanStack Virtual uses the new geometry.
  await nextTick()
  await nextAnimationFrame()
  const turn = conversationTurns.value[idx]
  const row = turn
    ? scroller.value?.querySelector(`[data-turn-id="${CSS.escape(turn.id)}"]`) as HTMLElement | null
    : null
  if (row) rowVirtualizer.value.resizeItem(idx, row.offsetHeight)
  rowVirtualizer.value.scrollToIndex(idx, { align: 'center', behavior: 'auto' })
  await nextTick()
  await nextAnimationFrame()
  return idx
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
    if (await mountEntryForMessage(targetId) === null) {
      console.warn('[MessageList] focus entry missing', targetId)
      chat.clearPendingFocusMessage()
      return
    }
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
      if (await mountEntryForMessage(targetMessageId) === null) return
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

function updateActiveBoardStickyState() {
  const el = scroller.value
  const board = activeBoard.value
  if (!el || !board) {
    activeBoardInlineScrollTop.value = null
    activeBoardIsSticky.value = false
    return
  }

  const inline = el.querySelector(
    `[data-active-parent-board-inline="${CSS.escape(board.storeKey)}"]`
  ) as HTMLElement | null
  if (inline) {
    activeBoardInlineScrollTop.value =
      inline.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop
  }
  activeBoardIsSticky.value = shouldStickActiveTaskBoard(
    el.scrollTop,
    activeBoardInlineScrollTop.value,
    true
  )
}

function onScroll() {
  const nearBottom = isNearBottom()
  wasNearBottomBeforeUpdate = nearBottom
  showScrollButton.value = !nearBottom
  updateActiveBoardStickyState()
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

function entryKey(entry: FlatEntry): string {
  if (entry.type === 'message') return `message-${entry.message.id}`
  if (entry.type === 'tool_run') {
    return `tool-run-${entry.items
      .map(item => item.kind === 'tools' ? item.group.id : item.message.id)
      .join('-')}`
  }
  return `task-board-${entry.storeKey}-${entry.anchorMessageId}`
}

function entryMessageStatuses(entry: FlatEntry): ChatMessage[] {
  if (entry.type === 'message') return [entry.message]
  if (entry.type === 'tool_run') {
    return entry.items.map(item => item.kind === 'tools' ? item.group.message : item.message)
  }
  return []
}

function entryHasStatus(entry: FlatEntry, statuses: ChatMessage['status'][]): boolean {
  return entryMessageStatuses(entry).some(message => statuses.includes(message.status))
}

function entryHasRunningTool(entry: FlatEntry): boolean {
  return entryMessageStatuses(entry).some(message =>
    message.toolCalls?.some(tool => tool.status === 'running' || tool.status === 'pending')
  )
}

function entryIsSummary(entry: FlatEntry): boolean {
  if (entry.type === 'task_board') return isTaskBoardTerminal(entry.document.meta?.status)
  return entry.type === 'message' && isCompressionSummaryMessage(entry.message)
}

function entryIsDelivery(entry: FlatEntry): boolean {
  return entry.type === 'message'
    && entry.message.role === 'assistant'
    && assistantHasDeliverableContent(entry.message)
    && !isEphemeralDesktopNoticeMessage(entry.message)
    && !isToolRunContinuityGlue(entry.message)
}

const activeBoard = computed(() => flatMessages.value.find(
  (entry): entry is Extract<FlatEntry, { type: 'task_board' }> =>
    entry.type === 'task_board' && entry.isActive && !isTaskBoardTerminal(entry.document.meta?.status)
) ?? null)

const turnSourceEntries = computed(() => flatMessages.value)
const expandedTurnIds = ref<Set<string>>(new Set())
const manuallyCollapsedTurnIds = ref<Set<string>>(new Set())
const conversationTurns = computed(() => buildConversationTurns(turnSourceEntries.value, {
  key: entryKey,
  userMessageId: entry => entry.type === 'message' && entry.message.role === 'user'
    ? entry.message.id
    : null,
  isActive: entry => entryHasStatus(entry, ['pending', 'streaming'])
    || entryHasRunningTool(entry)
    || (!!activeBoard.value && entryContainsMessageId(entry, activeBoard.value.anchorMessageId)),
  isFailed: entry => entryHasStatus(entry, ['error'])
    || (entry.type === 'task_board' && entry.document.meta?.status === 'failed'),
  isCancelled: entry => entryHasStatus(entry, ['cancelled'])
    || (entry.type === 'task_board' && entry.document.meta?.status === 'cancelled'),
  isSummary: entryIsSummary,
  isDelivery: entryIsDelivery
}))

function turnIsExpanded(turnId: string): boolean {
  if (manuallyCollapsedTurnIds.value.has(turnId)) return false
  return expandedTurnIds.value.has(turnId)
    || shouldAutoExpandTurn(conversationTurns.value, turnId)
}

function turnHasTaskBoard(turn: (typeof conversationTurns.value)[number]): boolean {
  return turn.entries.some(entry => entry.type === 'task_board')
}

const parentTaskBoardShellClass = 'task-board-sticky chat-column flex w-full justify-end py-1'

function resizeTurnRow(turnId: string) {
  const turnIndex = conversationTurns.value.findIndex(turn => turn.id === turnId)
  if (turnIndex < 0 || !scroller.value) return
  const row = scroller.value.querySelector(
    `[data-turn-id="${CSS.escape(turnId)}"]`
  ) as HTMLElement | null
  if (!row) return
  // Preserve every neighboring measurement; resetting the cache shifts the viewport.
  rowVirtualizer.value.resizeItem(turnIndex, row.offsetHeight)
}

function toggleTurn(turnId: string) {
  const expanded = new Set(expandedTurnIds.value)
  const collapsed = new Set(manuallyCollapsedTurnIds.value)
  if (turnIsExpanded(turnId)) {
    expanded.delete(turnId)
    collapsed.add(turnId)
  } else {
    collapsed.delete(turnId)
    expanded.add(turnId)
  }
  expandedTurnIds.value = expanded
  manuallyCollapsedTurnIds.value = collapsed
  void nextTick(() => resizeTurnRow(turnId))
}

function displayedTurnEntries(turn: (typeof conversationTurns.value)[number]): FlatEntry[] {
  return turnIsExpanded(turn.id) ? turn.entries : turn.collapsedEntries
}

function turnElapsedLabel(turnId: string): string {
  const messages = chat.current?.messages ?? []
  const userIndex = messages.findIndex(message => message.id === turnId && message.role === 'user')
  if (userIndex >= 0) {
    const nextUserOffset = messages
      .slice(userIndex + 1)
      .findIndex(message => message.role === 'user')
    const turnEnd = nextUserOffset >= 0 ? userIndex + 1 + nextUserOffset : messages.length
    const turnMessages = messages.slice(userIndex, turnEnd)
    const lastMessage = turnMessages[turnMessages.length - 1]
    const elapsed = lastMessage
      ? elapsedBetweenTimestamps(messages[userIndex]!.createdAt, lastMessage.createdAt)
      : null
    if (elapsed != null) return formatTurnElapsed(elapsed)
  }

  const conversationId = chat.currentId?.trim()
  return formatTurnElapsed(conversationId ? turnElapsedMs(conversationId, turnId) : null)
}

function expandTurnContainingMessage(messageId: string): number {
  const idx = conversationTurns.value.findIndex(turn =>
    turnContains(turn, entry => entryContainsMessageId(entry, messageId))
  )
  if (idx < 0) return -1
  const turn = conversationTurns.value[idx]!
  if (turn.hiddenCount > 0 && !turnIsExpanded(turn.id)) {
    manuallyCollapsedTurnIds.value = new Set(
      [...manuallyCollapsedTurnIds.value].filter(id => id !== turn.id)
    )
    expandedTurnIds.value = new Set(expandedTurnIds.value).add(turn.id)
  }
  return idx
}

watch(() => chat.currentId, () => {
  expandedTurnIds.value = new Set()
  manuallyCollapsedTurnIds.value = new Set()
  activeBoardInlineScrollTop.value = null
  activeBoardIsSticky.value = false
})

watch(
  () => activeBoard.value?.storeKey ?? null,
  () => {
    activeBoardInlineScrollTop.value = null
    activeBoardIsSticky.value = false
    void nextTick(updateActiveBoardStickyState)
  }
)

const rowVirtualizer = useVirtualizer<HTMLDivElement, HTMLDivElement>(computed(() => ({
  ...messageVirtualizerBaseOptions(
    conversationTurns.value.length,
    index => `turn-${conversationTurns.value[index]!.id}`
  ),
  getScrollElement: () => scroller.value
})))

watch(
  () => conversationTurns.value.map(turn => turn.id),
  (turnIds, previousTurnIds) => {
    const previousLastId = previousTurnIds[previousTurnIds.length - 1]
    if (!previousLastId || turnIds[turnIds.length - 1] === previousLastId) return
    if (!turnIds.includes(previousLastId) || expandedTurnIds.value.has(previousLastId)) return

    // The appended user turn removes the previous terminal turn's implicit expansion.
    // Patch that row's real compact height before the existing bottom-follow RAF runs.
    void nextTick(() => resizeTurnRow(previousLastId))
  }
)

const virtualRows = computed(() => rowVirtualizer.value.getVirtualItems())
const renderedRows = computed(() => virtualRows.value.flatMap(virtualRow => {
  const turn = conversationTurns.value[virtualRow.index]
  return turn ? [{ virtualRow, turn }] : []
}))

function setVirtualRowElement(node: Element | ComponentPublicInstance | null) {
  rowVirtualizer.value.measureElement(node instanceof HTMLDivElement ? node : null)
}

function spacingPixels(
  entry: FlatEntry,
  index: number,
  entries: FlatEntry[],
  followsCollapsedTurnIndicator = false
): number {
  return messageRowSpacingPixels(entrySpacing(entry, index, entries, followsCollapsedTurnIndicator))
}

function entrySpacing(
  entry: FlatEntry,
  index: number,
  entries: FlatEntry[],
  followsCollapsedTurnIndicator = false
): string {
  if (index === 0) return ''
  if (followsCollapsedTurnIndicator && index === 1) return 'mt-4'

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
    if (prev.type === 'message' && entryIsSummary(prev)) return 'mt-1.5'
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
  <div
    ref="scroller"
    class="chat-scroll-area h-full overflow-y-auto chat-shell pb-6"
    style="overflow-anchor: none"
    @scroll="onScroll"
  >
    <div
      v-if="activeBoard && activeBoardIsSticky"
      class="sticky top-0 z-30 h-0 overflow-visible"
    >
      <div :class="[parentTaskBoardShellClass, 'bg-background/95 backdrop-blur-sm']">
        <TaskBoardPanel
          :document="activeBoard.document"
          :is-active="activeBoard.isActive"
          :child-boards="chat.childBoardsForParent(chat.currentId, activeBoard.storeKey)"
          :conversation-id="chat.currentId"
        />
      </div>
    </div>

    <div
      class="chat-column relative w-full"
      :style="{ height: `${rowVirtualizer.getTotalSize()}px` }"
    >
      <div
        v-for="row in renderedRows"
        :key="String(row.virtualRow.key)"
        :ref="setVirtualRowElement"
        :data-index="row.virtualRow.index"
        :data-turn-id="row.turn.id"
        class="absolute left-0 top-0 w-full"
        :style="{
          transform: `translateY(${row.virtualRow.start}px)`,
          paddingTop: `${messageTurnSpacingPixels(row.virtualRow.index)}px`
        }"
      >
        <template
          v-for="(entry, entryIndex) in displayedTurnEntries(row.turn)"
          :key="entryKey(entry)"
        >
          <div
            :style="{
              paddingTop: `${spacingPixels(
                entry,
                entryIndex,
                displayedTurnEntries(row.turn),
                row.turn.hiddenCount > 0 && !turnIsExpanded(row.turn.id)
              )}px`
            }"
          >
            <div
              v-if="entry.type === 'message'"
              :data-message-id="entryPrimaryMessageId(entry)"
              :class="[
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
              :data-active-parent-board-inline="entry === activeBoard ? entry.storeKey : undefined"
              :class="[
                parentTaskBoardShellClass,
                'mb-1',
                entry === activeBoard && activeBoardIsSticky ? 'invisible' : ''
              ]"
            >
              <TaskBoardPanel
                :document="entry.document"
                :is-active="entry.isActive"
                :child-boards="chat.childBoardsForParent(chat.currentId, entry.storeKey)"
                :conversation-id="chat.currentId"
              />
            </div>

            <div
              v-if="row.turn.hiddenCount > 0 && (
                (entry.type === 'message' && entry.message.id === row.turn.id && !turnHasTaskBoard(row.turn))
                || (entry.type === 'task_board' && entry.anchorMessageId === row.turn.id)
              )"
              class="mt-1.5"
            >
              <button
                type="button"
                class="inline-flex items-center gap-1 py-1 text-xs font-medium text-muted/70 transition-colors hover:text-foreground"
                :aria-expanded="turnIsExpanded(row.turn.id)"
                @click="toggleTurn(row.turn.id)"
              >
                <ChevronDown v-if="turnIsExpanded(row.turn.id)" class="h-3 w-3" />
                <ChevronRight v-else class="h-3 w-3" />
                <span class="tabular-nums">{{ turnElapsedLabel(row.turn.id) }}</span>
              </button>
            </div>
          </div>
        </template>
      </div>
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

