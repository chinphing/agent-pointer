<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, provide, ref, watch } from 'vue'
import type { ComponentPublicInstance } from 'vue'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { ArrowDown, ChevronDown, ChevronRight, Plus, X } from 'lucide-vue-next'
import MessageRow from './message/MessageRow.vue'
import ToolMessageSegment from './message/assistant/ToolMessageSegment.vue'
import ToolRunGlueRow from './message/ToolRunGlueRow.vue'
import ContextCompressingMarker from './message/ContextCompressingMarker.vue'
import TaskBoardPanel from './TaskBoardPanel.vue'
import { useChatStore } from '../../stores/chat'
import { buildCompressionProgressLabel } from '../../lib/compressionMessage'
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
import { activeTurnStartedAt, formatTurnElapsed, resolveTurnElapsedMs } from '../../lib/turnElapsed'
import { shouldStickActiveTaskBoard } from '../../lib/taskBoardSticky'
import {
  countLlmInvocationRounds,
  MOBILE_VIEWPORT_MEDIA_QUERY,
  shouldShowMobileNewConversationButton
} from '../../lib/mobileChat'
import {
  PAGE_SEARCH_MARK_CLASS,
  clearSearchTextMarks,
  clearSidebarSearchTextMarks,
  highlightSearchText,
  highlightSidebarSearchText
} from '../../lib/sidebarSearchTextHighlight'
import { showScrollbarWhileScrolling } from '../../lib/autoHideScrollbar'
import {
  scrollTopForAnchor,
  turnOffsetInScroller,
  type MessageListScrollAnchor
} from '../../lib/messageListScrollAnchor'
import { nextFollowOutputAfterScroll } from '../../lib/messageListScrollFollow'

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

const contextCompressingLabel = computed(() => {
  const state = chat.contextCompressing
  if (!state) return ''
  return buildCompressionProgressLabel(state)
})

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
/** Prefetch older turns when within this distance of the top. */
const LOAD_OLDER_TOP_PX = 300
/**
 * After an older page lands, require the user to leave the top band once
 * before auto-prefetch can fire again. Prevents a restore-shortfall cascade
 * (scrollTop stays ≤ top threshold → load → OOM / white WebView).
 */
const LOAD_OLDER_LEAVE_TOP_PX = 400
/** Frames to re-apply scroll restore while the virtualizer catches up. */
const LOAD_OLDER_SETTLE_FRAMES = 4
/** Minimum gap between two history trims (avoid churn while scrolling). */
const TRIM_HISTORY_COOLDOWN_MS = 30_000
/** How often visible user messages are re-stamped and history is scanned. */
const TRIM_HISTORY_TICK_MS = 30_000
let lastScrollTs = 0
let touchStartY: number | null = null
/** Last observed scroller clientHeight; re-stick when chrome shrinks the viewport. */
let lastScrollerClientHeight = 0
let scrollerResizeObserver: ResizeObserver | null = null
let mobileMediaQuery: MediaQueryList | null = null
let lastScrollTop = 0
let olderLoadInFlight = false
/** Auto-prefetch is one-shot per visit to the top; re-arm after leaving it. */
let olderPrefetchArmed = true
let lastHistoryTrimAt = 0
let historyTrimTimer: ReturnType<typeof setInterval> | null = null
let focusAroundRequestedId: string | null = null
/** Max blank pull distance when there is no older page. */
const NO_OLDER_PULL_MAX_PX = 52
/** Show the hint copy once the pulled blank area is tall enough. */
const NO_OLDER_HINT_REVEAL_PX = 28
/** Current top pull gap (px); 0 when not pulling. */
const noOlderPullPx = ref(0)
let noOlderPullTouchY: number | null = null
let noOlderPullWheelReleaseTimer: ReturnType<typeof setTimeout> | null = null

const currentMessagePage = computed(() => {
  const id = chat.currentId?.trim()
  if (!id) return null
  return chat.messagePageState(id)
})
const showNoOlderPullHint = computed(() => noOlderPullPx.value >= NO_OLDER_HINT_REVEAL_PX)

function canPullNoOlderHint(el: HTMLElement): boolean {
  return currentMessagePage.value?.hasMoreOlder === false
    && el.scrollTop <= 1
    && programmaticScrollDepth === 0
}

function setNoOlderPullPx(px: number) {
  noOlderPullPx.value = Math.max(0, Math.min(NO_OLDER_PULL_MAX_PX, px))
}

function releaseNoOlderPull() {
  noOlderPullPx.value = 0
  noOlderPullTouchY = null
  if (noOlderPullWheelReleaseTimer != null) {
    clearTimeout(noOlderPullWheelReleaseTimer)
    noOlderPullWheelReleaseTimer = null
  }
}

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
  // Keep lastScrollTop in sync so the next user drag is detected as scrollingUp
  // (otherwise lastScrollTop stays 0 after mount and scrollbar detach fails).
  lastScrollTop = el.scrollTop
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
  const el = scroller.value
  if (!el) return
  if (event.deltaY > 0 || el.scrollTop > 1) {
    if (noOlderPullPx.value > 0) releaseNoOlderPull()
    return
  }
  // Already at top with no older pages: grow a blank gap and show the hint.
  if (!canPullNoOlderHint(el) || event.deltaY >= 0) return
  setNoOlderPullPx(noOlderPullPx.value + Math.min(20, -event.deltaY * 0.4))
  if (noOlderPullWheelReleaseTimer != null) clearTimeout(noOlderPullWheelReleaseTimer)
  noOlderPullWheelReleaseTimer = setTimeout(() => {
    noOlderPullWheelReleaseTimer = null
    releaseNoOlderPull()
  }, 450)
}

function onTouchStart(event: TouchEvent) {
  touchStartY = event.touches[0]?.clientY ?? null
  const el = scroller.value
  noOlderPullTouchY =
    el && canPullNoOlderHint(el) ? touchStartY : null
}

function onTouchMove(event: TouchEvent) {
  if (touchStartY == null) return
  const y = event.touches[0]?.clientY
  if (y == null) return
  // Finger moves down → content scrolls toward older messages.
  if (y - touchStartY > 8) unpinFollowOutput()
  const el = scroller.value
  if (noOlderPullTouchY == null || !el) return
  if (!canPullNoOlderHint(el)) {
    releaseNoOlderPull()
    return
  }
  const pull = y - noOlderPullTouchY
  setNoOlderPullPx(pull > 0 ? pull * 0.45 : 0)
}

function onTouchEnd() {
  touchStartY = null
  releaseNoOlderPull()
}

function updateMobileViewport() {
  isMobileViewport.value = window.matchMedia(MOBILE_VIEWPORT_MEDIA_QUERY).matches
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
  // Recover a stale older-load flag left after a crashed / remounted request.
  chat.clearStuckOlderLoading()
  mobileMediaQuery = window.matchMedia(MOBILE_VIEWPORT_MEDIA_QUERY)
  updateMobileViewport()
  mobileMediaQuery.addEventListener('change', updateMobileViewport)
  const el = scroller.value
  if (el && typeof ResizeObserver !== 'undefined') {
    lastScrollerClientHeight = el.clientHeight
    scrollerResizeObserver = new ResizeObserver(() => {
      const scrollerEl = scroller.value
      if (!scrollerEl || !shouldFollowOutput()) return
      const previousHeight = lastScrollerClientHeight
      const nextHeight = scrollerEl.clientHeight
      if (nextHeight === previousHeight) return
      lastScrollerClientHeight = nextHeight
      if (nextHeight >= previousHeight) return
      // Keep the same visual messages in place while the composer grows. Unlike
      // scrollToIndex(), this compensates only for the viewport-height delta and
      // never remeasures or jumps to a virtualized row.
      beginProgrammaticScroll()
      scrollerEl.scrollTop += previousHeight - nextHeight
      endProgrammaticScroll()
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
  historyTrimTimer = setInterval(() => {
    stampVisibleUserMessagesViewed()
    maybeTrimConversationHistory()
  }, TRIM_HISTORY_TICK_MS)
})

onBeforeUnmount(() => {
  stopElapsedTicker()
  releaseNoOlderPull()
  document.removeEventListener('keydown', onNewConversationConfirmationKeydown)
  mobileMediaQuery?.removeEventListener('change', updateMobileViewport)
  mobileMediaQuery = null
  if (historyTrimTimer != null) {
    clearInterval(historyTrimTimer)
    historyTrimTimer = null
  }
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
  releaseNoOlderPull()
  await nextTick()
  rowVirtualizer.value.measure()
  toBottom({ settle: true })
  stampVisibleUserMessagesViewed()
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
    if (focusAroundRequestedId !== targetId) {
      focusAroundRequestedId = targetId
      console.info('[MessageList] focus target missing; loading around message', targetId)
      void chat.ensureMessagesAround(pending.conversationId, targetId).then(ok => {
        if (!ok) {
          console.warn('[MessageList] focus message not found after around hydrate', targetId)
          chat.clearPendingFocusMessage()
          focusAroundRequestedId = null
        }
      })
      return
    }
    console.warn('[MessageList] focus message not found after around hydrate', targetId)
    chat.clearPendingFocusMessage()
    focusAroundRequestedId = null
    return
  }
  focusAroundRequestedId = null

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

async function loadOlderWithScrollAnchor() {
  const el = scroller.value
  if (!el || locatingFocus.value) return
  // Heal a stale store flag (no in-flight request) so the button is not stuck.
  chat.clearStuckOlderLoading()
  const page = currentMessagePage.value
  // Strict serial: local lock OR store lock — previous page must fully finish
  // (IPC + prepend + scroll settle) before another load can start.
  if (!page?.hasMoreOlder || page.loadingOlder || olderLoadInFlight) return

  olderLoadInFlight = true
  // Disarm auto-prefetch until the user leaves the top band.
  olderPrefetchArmed = false
  followOutput = false
  const anchor = captureVisibleTurnAnchor()
  beginProgrammaticScroll()
  try {
    const added = await chat.loadOlderMessages()
    if (!added) return
    // Remeasured prepended rows change totalSize over several frames; re-pin
    // the same turn id + viewport offset each frame (not scrollHeight delta).
    for (let i = 0; i < LOAD_OLDER_SETTLE_FRAMES; i += 1) {
      await nextTick()
      await nextAnimationFrame()
      if (!restoreVisibleTurnAnchor(anchor)) break
    }
    console.info('[MessageList] restored scroll after older page', {
      turnId: anchor?.turnId,
      offsetPx: anchor?.offsetPx,
      scrollTop: scroller.value?.scrollTop
    })
  } finally {
    // Only release the lock after settle — prevents cascading loads while
    // height/scroll are still catching up.
    olderLoadInFlight = false
    endProgrammaticScroll()
  }
}

/** Snapshot the topmost visible turn so prepend/trim can re-pin the viewport. */
function captureVisibleTurnAnchor(): MessageListScrollAnchor | null {
  const el = scroller.value
  const firstRow = virtualRows.value[0]
  if (!el || !firstRow) return null
  const turn = conversationTurns.value[firstRow.index]
  if (!turn) return null
  const turnEl = el.querySelector(
    `[data-turn-id="${CSS.escape(turn.id)}"]`
  ) as HTMLElement | null
  if (turnEl) {
    return {
      turnId: turn.id,
      offsetPx: turnOffsetInScroller(el, turnEl)
    }
  }
  // Fallback when the row exists in the virtual window but DOM is not ready.
  return {
    turnId: turn.id,
    offsetPx: firstRow.start - el.scrollTop
  }
}

/** Re-apply a captured turn anchor after the virtualizer layout changes. */
function restoreVisibleTurnAnchor(anchor: MessageListScrollAnchor | null): boolean {
  const el = scroller.value
  if (!el || !anchor) return false
  const idx = conversationTurns.value.findIndex(turn => turn.id === anchor.turnId)
  if (idx < 0) {
    console.warn('[MessageList] scroll anchor turn missing after layout change', anchor.turnId)
    return false
  }
  const offsetPair = rowVirtualizer.value.getOffsetForIndex(idx, 'start')
  if (!offsetPair) {
    rowVirtualizer.value.scrollToIndex(idx, { align: 'start', behavior: 'auto' })
    lastScrollTop = el.scrollTop
    return true
  }
  const [turnStartPx] = offsetPair
  el.scrollTop = scrollTopForAnchor(turnStartPx, anchor.offsetPx)
  lastScrollTop = el.scrollTop
  return true
}

function maybePrefetchOlder() {
  const el = scroller.value
  if (!el || programmaticScrollDepth > 0 || locatingFocus.value) return
  if (olderLoadInFlight || currentMessagePage.value?.loadingOlder) return
  const atTop = el.scrollTop <= LOAD_OLDER_TOP_PX
  if (!atTop && el.scrollTop > LOAD_OLDER_LEAVE_TOP_PX) {
    olderPrefetchArmed = true
  }
  const scrollingUp = el.scrollTop < lastScrollTop - 1
  lastScrollTop = el.scrollTop
  if (!atTop || !scrollingUp || !olderPrefetchArmed) return
  void loadOlderWithScrollAnchor()
}

/** Stamp every user message currently inside the virtual viewport as "viewed". */
function stampVisibleUserMessagesViewed() {
  const convId = chat.currentId?.trim()
  if (!convId) return
  for (const row of virtualRows.value) {
    const turn = conversationTurns.value[row.index]
    if (!turn) continue
    for (const entry of turn.entries) {
      if (entry.type !== 'message' || entry.message.role !== 'user') continue
      chat.markUserMessageViewed(convId, entry.message.id)
    }
  }
}

/**
 * Release old in-memory history of the *current* conversation when user
 * messages above the viewport have not been viewed within the stale window.
 * SQLite keeps the rows; scrolling back up near the top reloads them through
 * `loadOlderMessages` because `oldestPosition` was advanced.
 */
function maybeTrimConversationHistory() {
  const convId = chat.currentId?.trim()
  if (!convId || locatingFocus.value || programmaticScrollDepth > 0) return
  if (olderLoadInFlight) return
  if (Date.now() - lastHistoryTrimAt < TRIM_HISTORY_COOLDOWN_MS) return
  const anchor = captureVisibleTurnAnchor()
  const removed = chat.trimConversationHistory(convId)
  lastHistoryTrimAt = Date.now()
  if (removed > 0 && anchor) {
    // Rows above the viewport were dropped; re-pin the same turn + offset.
    void (async () => {
      await nextTick()
      await nextAnimationFrame()
      restoreVisibleTurnAnchor(anchor)
    })()
  }
}

function onScroll(event: Event) {
  showScrollbarWhileScrolling(event)
  const el = scroller.value
  if (el && el.scrollTop > 2 && noOlderPullPx.value > 0) {
    releaseNoOlderPull()
  }
  const distance = distanceFromBottom()
  if (programmaticScrollDepth === 0) {
    // Scrollbar drag only emits scroll (not wheel). Detect scrollTop moving
    // toward older content and unpin immediately — otherwise follow stays on
    // inside the attach/detach band and scheduleToBottom fights the thumb.
    const scrollingUp = el != null && el.scrollTop < lastScrollTop - 1
    followOutput = nextFollowOutputAfterScroll({
      followOutput,
      distanceFromBottom: distance,
      scrollingUp,
      attachPx: ATTACH_BOTTOM_PX,
      detachPx: DETACH_BOTTOM_PX
    })
  }
  showScrollButton.value = !followOutput
  updateActiveBoardStickyState()
  maybePrefetchOlder()
  stampVisibleUserMessagesViewed()
  maybeTrimConversationHistory()
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
    cache: layoutCacheHold,
    collapseActiveTurns: settings.userSettings.collapseProcessByDefault === true
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

// Live ticking clock for the running turn's elapsed label. The interval runs
// only while at least one turn is still active; otherwise nothing re-renders.
// (With collapseProcessByDefault off, active turns have hiddenCount 0 so the
// chip is absent mid-run — same as before that setting existed.)
const nowTick = ref(Date.now())
let elapsedTicker: ReturnType<typeof setInterval> | null = null
const hasActiveConversationTurn = computed(() =>
  conversationTurns.value.some(turn => turn.state === 'active')
)
watch(hasActiveConversationTurn, active => {
  if (active && elapsedTicker == null) {
    nowTick.value = Date.now()
    elapsedTicker = setInterval(() => {
      nowTick.value = Date.now()
    }, 1000)
  } else if (!active && elapsedTicker != null) {
    clearInterval(elapsedTicker)
    elapsedTicker = null
  }
}, { immediate: true })

function stopElapsedTicker() {
  if (elapsedTicker != null) {
    clearInterval(elapsedTicker)
    elapsedTicker = null
  }
}

function turnIsExpanded(turnId: string): boolean {
  if (manuallyCollapsedTurnIds.value.has(turnId)) return false
  return expandedTurnIds.value.has(turnId)
    || (!settings.userSettings.collapseProcessByDefault && shouldAutoExpandTurn(conversationTurns.value, turnId))
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
  const conversationId = chat.currentId?.trim()
  // Running turn: render a live duration driven by the 1s ticker (reading
  // nowTick keeps the label reactive even when nothing else changes).
  const startedAt = conversationId ? activeTurnStartedAt(conversationId, turnId) : null
  if (startedAt != null) {
    return formatTurnElapsed(Math.max(0, nowTick.value - startedAt))
  }
  const messages = chat.current?.messages ?? []
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
      class="chat-scroll-area auto-hide-scrollbar h-full overflow-y-auto chat-shell pb-6"
      style="overflow-anchor: none"
      @scroll="onScroll"
      @wheel="onWheel"
      @touchstart.passive="onTouchStart"
      @touchmove.passive="onTouchMove"
      @touchend="onTouchEnd"
      @touchcancel="onTouchEnd"
    >
    <div
      class="flex shrink-0 items-center justify-center overflow-hidden"
      :style="{ height: `${noOlderPullPx}px` }"
      aria-hidden="true"
    >
      <p
        v-if="showNoOlderPullHint"
        class="text-xs text-muted"
        role="status"
      >
        没有更早的消息
      </p>
    </div>

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
                :content-only="entry.contentOnly"
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

    <div
      v-if="contextCompressingLabel"
      class="chat-column tool-segments pt-1"
    >
      <ContextCompressingMarker :label="contextCompressingLabel" />
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
      v-if="showScrollButton"
      type="button"
      class="absolute bottom-4 right-4 z-40 h-10 w-10 rounded-full panel shadow-lg flex items-center justify-center cursor-pointer hover:bg-hover transition"
      @click="toBottom({ settle: true })"
      title="滚动到底部"
    >
      <ArrowDown class="w-5 h-5 text-foreground" />
    </button>
  </div>
</template>

