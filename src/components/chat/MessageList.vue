<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, provide, ref, watch } from 'vue'
import type { ComponentPublicInstance } from 'vue'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { ArrowDown, ChevronDown, ChevronRight, Plus, X } from 'lucide-vue-next'
import MessageRow from './message/MessageRow.vue'
import ToolMessageSegment from './message/assistant/ToolMessageSegment.vue'
import ToolRunGlueRow from './message/ToolRunGlueRow.vue'
import TaskBoardPanel from './TaskBoardPanel.vue'
import { useChatStore } from '../../stores/chat'
import { useSettingsStore } from '../../stores/settings'
import { uiForMessageAgent, useAgentsCatalog } from '../../composables/useAgentUi'
import { visibleToolCalls } from '../../lib/messageTooling'
import type { ChatMessage, ToolCall } from '../../types/chat'
import {
  isEphemeralDesktopNoticeMessage,
  isToolOnlyAssistantMessage
} from '../../lib/assistantMessageKind'
import { isCompressionSummaryMessage } from '../../lib/compressionMessage'
import { shouldShowGlueMessage } from '../../lib/threadLayoutGlue'
import { messageRowSpacingPixels, messageTurnSpacingPixels, messageVirtualizerBaseOptions } from '../../lib/messageVirtualization'
import {
  buildMessageListLayout,
  entryContainsMessageId,
  entryKey,
  type FlatEntry,
  type MessageListLayoutCache
} from '../../lib/messageListLayout'
import { shouldAutoExpandTurn, turnContains } from '../../lib/conversationTurns'
import { formatTurnElapsed, resolveTurnElapsedMs } from '../../lib/turnElapsed'
import { shouldStickActiveTaskBoard } from '../../lib/taskBoardSticky'
import {
  countLlmInvocationRounds,
  shouldShowMobileNewConversationButton
} from '../../lib/mobileChat'
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
const newConversationConfirmOpen = ref(false)
const isMobileViewport = ref(false)
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
//
// Sticky follow: while the user stays at the bottom, streaming appends keep
// scrolling down. Any intentional scroll-up pins them away until they return
// essentially to the bottom (hysteresis) or click the jump button.
let followOutput = true
let programmaticScrollDepth = 0
let scrollFrame: number | null = null
/** Minimum wall-clock gap between two programmatic scroll-to-bottom calls. */
const SCROLL_MIN_INTERVAL_MS = 80
/** Must be this close to resume auto-follow after the user scrolled away. */
const ATTACH_BOTTOM_PX = 8
/** Scroll this far from bottom before onScroll alone detaches follow. */
const DETACH_BOTTOM_PX = 48
let lastScrollTs = 0
let touchStartY: number | null = null
/** Last observed scroller clientHeight; re-stick when chrome shrinks the viewport. */
let lastScrollerClientHeight = 0
let scrollerResizeObserver: ResizeObserver | null = null
let mobileMediaQuery: MediaQueryList | null = null

function distanceFromBottom(): number {
  const el = scroller.value
  if (!el) return 0
  return el.scrollHeight - el.scrollTop - el.clientHeight
}

function beginProgrammaticScroll() {
  programmaticScrollDepth += 1
}

function endProgrammaticScroll() {
  // Double rAF so scroll events from scrollToIndex settle before we re-enable
  // user-driven follow updates.
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      programmaticScrollDepth = Math.max(0, programmaticScrollDepth - 1)
    })
  })
}

function shouldFollowOutput(): boolean {
  return followOutput && !locatingFocus.value
}

/**
 * Align to the last turn, then force true scroll bottom so virtualizer
 * `paddingEnd` and scroller `pb-*` are actually visible (scrollToIndex align:end
 * only lines up the last item, leaving bottom padding below the fold).
 */
function stickScrollerToBottom() {
  if (conversationTurns.value.length === 0) return
  rowVirtualizer.value.scrollToIndex(conversationTurns.value.length - 1, {
    align: 'end',
    behavior: 'auto'
  })
  const el = scroller.value
  if (!el) return
  el.scrollTop = Math.max(0, el.scrollHeight - el.clientHeight)
}

function scheduleToBottom() {
  if (!shouldFollowOutput()) return
  if (scrollFrame != null) return
  scrollFrame = requestAnimationFrame(() => {
    scrollFrame = null
    if (!shouldFollowOutput()) return
    const now = performance.now()
    if (now - lastScrollTs < SCROLL_MIN_INTERVAL_MS) return
    lastScrollTs = now
    toBottom()
  })
}

function toBottom(options?: { settle?: boolean }) {
  if (locatingFocus.value || conversationTurns.value.length === 0) return
  followOutput = true
  showScrollButton.value = false
  const settle = options?.settle === true
  beginProgrammaticScroll()
  void (async () => {
    try {
      await nextTick()
      stickScrollerToBottom()
      // Switch/mount: row heights start as estimates; measureElement then
      // corrects totalSize and would otherwise leave a visible jump or clip
      // the last bubble against the composer edge.
      if (settle) {
        await nextAnimationFrame()
        stickScrollerToBottom()
        await nextAnimationFrame()
        stickScrollerToBottom()
      }
    } finally {
      endProgrammaticScroll()
    }
  })()
}

function unpinFollowOutput() {
  if (!followOutput) return
  followOutput = false
  showScrollButton.value = true
}

function onWheel(event: WheelEvent) {
  // Trackpad / mouse wheel up = read older messages → stop fighting the user.
  if (event.deltaY < 0) unpinFollowOutput()
}

function onTouchStart(event: TouchEvent) {
  touchStartY = event.touches[0]?.clientY ?? null
}

function onTouchMove(event: TouchEvent) {
  if (touchStartY == null) return
  const y = event.touches[0]?.clientY
  if (y == null) return
  // Finger moves down → content scrolls toward older messages.
  if (y - touchStartY > 8) unpinFollowOutput()
}

function onTouchEnd() {
  touchStartY = null
}

function updateMobileViewport() {
  isMobileViewport.value = window.matchMedia('(max-width: 767.98px)').matches
}

const showMobileNewConversationButton = computed(() =>
  shouldShowMobileNewConversationButton(
    isMobileViewport.value,
    countLlmInvocationRounds(chat.current?.messages ?? [])
  )
)

function openNewConversationConfirmation() {
  newConversationConfirmOpen.value = true
  document.addEventListener('keydown', onNewConversationConfirmationKeydown)
}

function closeNewConversationConfirmation() {
  newConversationConfirmOpen.value = false
  document.removeEventListener('keydown', onNewConversationConfirmationKeydown)
}

function confirmNewConversation() {
  chat.newConversation()
  closeNewConversationConfirmation()
}

function onNewConversationConfirmationKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') closeNewConversationConfirmation()
}

onMounted(() => {
  mobileMediaQuery = window.matchMedia('(max-width: 767.98px)')
  updateMobileViewport()
  mobileMediaQuery.addEventListener('change', updateMobileViewport)
  const el = scroller.value
  if (el && typeof ResizeObserver !== 'undefined') {
    lastScrollerClientHeight = el.clientHeight
    scrollerResizeObserver = new ResizeObserver(() => {
      const scrollerEl = scroller.value
      if (!scrollerEl || !shouldFollowOutput()) return
      if (scrollerEl.clientHeight === lastScrollerClientHeight) return
      lastScrollerClientHeight = scrollerEl.clientHeight
      // Composer / ChangeSummary / draft chrome changing height shrinks the list
      // viewport after the first stick — re-pin while following.
      scheduleToBottom()
    })
    scrollerResizeObserver.observe(el)
  }
  void nextTick(() => {
    toBottom({ settle: true })
    // Sidebar search focus can already be pending when this component is mounted
    // after the hydration skeleton was replaced. In that case the watcher below
    // registered too late to observe the state change, so retry from mounted.
    void tryLocatePendingFocus()
  })
})

onBeforeUnmount(() => {
  document.removeEventListener('keydown', onNewConversationConfirmationKeydown)
  mobileMediaQuery?.removeEventListener('change', updateMobileViewport)
  mobileMediaQuery = null
  if (scrollFrame != null) {
    cancelAnimationFrame(scrollFrame)
    scrollFrame = null
  }
  scrollerResizeObserver?.disconnect()
  scrollerResizeObserver = null
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
  followOutput = true
  await nextTick()
  rowVirtualizer.value.measure()
  toBottom({ settle: true })
})

watch(() => chat.current?.messages.length, () => {
  if (!shouldFollowOutput()) return

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
  if (shouldFollowOutput()) scheduleToBottom()
})

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
  const distance = distanceFromBottom()
  if (programmaticScrollDepth === 0) {
    // Hysteresis: wheel/touch may unpin while still within DETACH_BOTTOM_PX.
    // Only resume follow when essentially at the bottom again.
    if (distance <= ATTACH_BOTTOM_PX) {
      followOutput = true
    } else if (distance > DETACH_BOTTOM_PX) {
      followOutput = false
    }
  }
  showScrollButton.value = !followOutput
  updateActiveBoardStickyState()
}

function isTaskBoardTerminal(status: string | undefined): boolean {
  const s = (status ?? '').trim()
  return s === 'completed' || s === 'failed'
}

function assistantMessageHadTools(entry: FlatEntry): boolean {
  if (entry.type !== 'message' || entry.message.role !== 'assistant') return false
  return (entry.message.toolCalls?.length ?? 0) > 0 || (entry.trailingToolGroups?.length ?? 0) > 0
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

function shouldShowThreadGlue(message: ChatMessage): boolean {
  return shouldShowGlueMessage(message, settings.settings)
}

/**
 * Completed-turn structure cache (non-reactive on purpose).
 * Updating a ref inside this computed would retrigger itself.
 */
let layoutCacheHold: MessageListLayoutCache | null = null

const messageListLayout = computed(() => {
  void chat.taskBoards
  void settings.settings
  void agentsCatalog.value
  const result = buildMessageListLayout({
    conversationId: chat.currentId,
    messages: chat.current?.messages ?? [],
    deps: {
      boardsForMessage: messageId => chat.parentBoardsBoundToMessage(chat.currentId, messageId),
      visibleToolCallsFor: message => visibleToolsForMessage(message, message.toolCalls ?? []),
      shouldShowGlue: shouldShowThreadGlue
    },
    cache: layoutCacheHold
  })
  layoutCacheHold = result.cache
  return result
})

const flatMessages = computed(() => messageListLayout.value.entries)
const conversationTurns = computed(() => messageListLayout.value.turns)

const activeBoard = computed(() => flatMessages.value.find(
  (entry): entry is Extract<FlatEntry, { type: 'task_board' }> =>
    entry.type === 'task_board' && entry.isActive && !isTaskBoardTerminal(entry.document.meta?.status)
) ?? null)

const expandedTurnIds = ref<Set<string>>(new Set())
const manuallyCollapsedTurnIds = ref<Set<string>>(new Set())

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
  const conversationId = chat.currentId?.trim()
  const userIndex = messages.findIndex(message => message.id === turnId && message.role === 'user')
  let userCreatedAt: number | null = null
  let lastMessageCreatedAt: number | null = null
  if (userIndex >= 0) {
    const nextUserOffset = messages
      .slice(userIndex + 1)
      .findIndex(message => message.role === 'user')
    const turnEnd = nextUserOffset >= 0 ? userIndex + 1 + nextUserOffset : messages.length
    const turnMessages = messages.slice(userIndex, turnEnd)
    const lastMessage = turnMessages[turnMessages.length - 1]
    userCreatedAt = messages[userIndex]!.createdAt
    lastMessageCreatedAt = lastMessage?.createdAt ?? null
  }
  return formatTurnElapsed(
    resolveTurnElapsedMs({
      conversationId,
      turnId,
      userCreatedAt,
      lastMessageCreatedAt
    })
  )
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
  layoutCacheHold = null
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

// Estimate → measure (and late-expanding last turns) change totalSize after the
// first stick on conversation switch. Re-pin while the user is still following.
watch(
  () => rowVirtualizer.value.getTotalSize(),
  () => {
    if (!shouldFollowOutput()) return
    beginProgrammaticScroll()
    void nextTick(() => {
      stickScrollerToBottom()
      endProgrammaticScroll()
    })
  }
)

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
    if (prev.type === 'message' && isCompressionSummaryMessage(prev.message)) return 'mt-1.5'
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
  <div class="relative h-full min-h-0">
    <div
      ref="scroller"
      class="chat-scroll-area h-full overflow-y-auto chat-shell pb-6"
      style="overflow-anchor: none"
      @scroll="onScroll"
      @wheel="onWheel"
      @touchstart.passive="onTouchStart"
      @touchmove.passive="onTouchMove"
      @touchend="onTouchEnd"
      @touchcancel="onTouchEnd"
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
  </div>

    <button
      v-if="showMobileNewConversationButton"
      type="button"
      class="absolute bottom-4 left-4 z-40 h-10 w-10 rounded-full panel shadow-lg flex items-center justify-center cursor-pointer hover:bg-hover transition md:hidden"
      title="新建会话"
      aria-label="新建会话"
      @click="openNewConversationConfirmation"
    >
      <Plus class="w-5 h-5 text-foreground" />
    </button>

    <Teleport to="body">
      <div
        v-if="newConversationConfirmOpen"
        class="fixed inset-0 z-[220] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-conversation-confirm-title"
        @click.self="closeNewConversationConfirmation"
      >
        <div class="w-full max-w-md rounded-2xl border border-border bg-[hsl(var(--card-elevated))] p-5 shadow-2xl">
          <h2 id="new-conversation-confirm-title" class="text-base font-semibold text-foreground">新建会话？</h2>
          <p class="mt-2 text-[13px] leading-relaxed text-muted">
            将切换到一个新的会话。当前会话会保留在历史记录中，不会丢失。
          </p>
          <div class="mt-5 flex justify-end gap-2">
            <button
              type="button"
              class="h-9 rounded-lg bg-hover px-4 text-sm text-foreground transition-opacity hover:opacity-90"
              @click="closeNewConversationConfirmation"
            >
              取消
            </button>
            <button
              type="button"
              class="h-9 rounded-lg bg-accent px-4 text-sm font-medium text-white transition-opacity hover:opacity-95"
              @click="confirmNewConversation"
            >
              确认新建
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <button
      type="button"
      class="absolute bottom-4 right-4 z-40 h-10 w-10 rounded-full panel shadow-lg flex items-center justify-center cursor-pointer hover:bg-hover transition"
      @click="toBottom({ settle: true })"
      title="滚动到底部"
    >
      <ArrowDown class="w-5 h-5 text-foreground" />
    </button>
  </div>
</template>

