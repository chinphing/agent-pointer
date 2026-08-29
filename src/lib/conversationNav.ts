import type { ChatMessage, ConversationOutlineItem } from '../types/chat'
import { isCompressionSummaryMessage } from './compressionMessage'
import { isInternalRetryUserMessage, isScreenInjectUserMessage } from './threadLayoutGlue'

/** In-chat 导航 one-line label (including `…` when truncated). */
export const CONVERSATION_NAV_PREVIEW_CHARS = 36

/** How many ticks away the Codex fisheye falls off. */
export const CONVERSATION_NAV_FISHEYE_RADIUS = 4.2
/** Hit-slot height for each tick button. */
export const CONVERSATION_NAV_TICK_SLOT_PX = 6
export const CONVERSATION_NAV_TICK_GAP_PX = 1
/** Scroll the rail after about this many ticks are visible. */
export const CONVERSATION_NAV_MAX_VISIBLE_TICKS = 30
const NAV_LIST_PADDING_Y_PX = 8

export function conversationNavMaxHeightPx(): number {
  const n = CONVERSATION_NAV_MAX_VISIBLE_TICKS
  return n * CONVERSATION_NAV_TICK_SLOT_PX + (n - 1) * CONVERSATION_NAV_TICK_GAP_PX + NAV_LIST_PADDING_Y_PX
}

/** Transcript scroller under the nav overlay (sibling of `MessageList`). */
export function conversationNavTranscriptScroller(fromAside: HTMLElement | null): HTMLElement | null {
  const root = fromAside?.parentElement
  if (!root) return null
  const el = root.querySelector('.chat-scroll-area')
  return el instanceof HTMLElement ? el : null
}

/** Treat ticks as revealed when they overlap the viewport by this much. */
export const CONVERSATION_NAV_SCROLL_EDGE_PX = 2

export type ConversationNavScrollAffordances = {
  up: boolean
  down: boolean
}

/** Arrows only when a tick is actually clipped above/below the rail viewport. */
export function conversationNavScrollAffordances(input: {
  firstTickTop: number
  lastTickBottom: number
  viewportTop: number
  viewportBottom: number
}): ConversationNavScrollAffordances {
  const edge = CONVERSATION_NAV_SCROLL_EDGE_PX
  return {
    up: input.firstTickTop < input.viewportTop - edge,
    down: input.lastTickBottom > input.viewportBottom + edge
  }
}

/** One click on a nav arrow pans about one visible tick window. */
export function conversationNavPageDelta(clientHeight: number): number {
  return Math.max(48, Math.round(clientHeight * 0.85))
}

/**
 * Float tick index whose center sits under `pointerY`.
 * Uses the first tick's top + fixed slot/gap — not `scrollHeight`, which
 * includes padding and puts the peak about one slot off.
 */
export function conversationNavFocusFromPointer(input: {
  pointerY: number
  firstTickTop: number
  tickCount: number
  tickSlotPx?: number
  tickGapPx?: number
}): number {
  const n = input.tickCount
  if (n <= 1) return 0
  const slot = input.tickSlotPx ?? CONVERSATION_NAV_TICK_SLOT_PX
  const gap = input.tickGapPx ?? CONVERSATION_NAV_TICK_GAP_PX
  const stride = slot + gap
  const focus = (input.pointerY - input.firstTickTop - slot / 2) / stride
  return Math.min(n - 1, Math.max(0, focus))
}

export type ConversationNavTickVisual = {
  widthPx: number
  heightPx: number
  opacity: number
}

export function conversationNavPreview(
  content: string,
  maxChars = CONVERSATION_NAV_PREVIEW_CHARS
): string {
  const collapsed = content.trim().split(/\s+/).filter(Boolean).join(' ')
  if (!collapsed) return '（无文字）'
  const chars = [...collapsed]
  if (chars.length <= maxChars) return collapsed
  return `${chars.slice(0, Math.max(0, maxChars - 1)).join('')}…`
}

/** Hover fisheye: distance 0 is the pointer; farther ticks shrink and fade. */
export function conversationNavFisheye(distance: number): ConversationNavTickVisual {
  const t = Math.min(1, Math.max(0, distance) / CONVERSATION_NAV_FISHEYE_RADIUS)
  const k = (1 - t) * (1 - t)
  return {
    widthPx: 4 + 12 * k,
    heightPx: 1.25 + 1.25 * k,
    opacity: 0.2 + 0.8 * k
  }
}

export function conversationNavRestTick(isActive: boolean): ConversationNavTickVisual {
  if (isActive) {
    return { widthPx: 6, heightPx: 2, opacity: 0.92 }
  }
  return { widthPx: 5, heightPx: 1.5, opacity: 0.28 }
}

function isOptimisticNavUserMessage(message: ChatMessage): boolean {
  if (message.role !== 'user' || !message.id) return false
  if (message.anchorMessageId?.trim()) return false
  if (isScreenInjectUserMessage(message) || isInternalRetryUserMessage(message)) return false
  if (isCompressionSummaryMessage(message)) return false
  return true
}

/** Keep API order; append in-memory user turns not yet in SQLite outline. */
export function mergeConversationNavItems(
  fromApi: ConversationOutlineItem[],
  messages: readonly ChatMessage[]
): ConversationOutlineItem[] {
  const known = new Set(fromApi.map(item => item.messageId))
  const extra: ConversationOutlineItem[] = []
  for (const message of messages) {
    if (!isOptimisticNavUserMessage(message) || known.has(message.id)) continue
    extra.push({
      messageId: message.id,
      preview: conversationNavPreview(message.content)
    })
    known.add(message.id)
  }
  return extra.length === 0 ? fromApi : [...fromApi, ...extra]
}

/**
 * At the transcript's real bottom the 18% marker can still sit in an earlier
 * turn (short lists are top-weighted). Pin to the last loaded turn so the
 * current tick matches "cannot scroll further".
 */
export function conversationNavVisibleMessageId(input: {
  atBottom: boolean
  lastLoadedTurnId: string | null
  markerTurnId: string | null
}): string | null {
  if (input.atBottom && input.lastLoadedTurnId) return input.lastLoadedTurnId
  return input.markerTurnId
}

/**
 * Last tick while the loaded window still has newer turns must load the real
 * tail. Around-merge would treat the around-window's last user as already
 * loaded and never reveal later conversation content.
 */
export function conversationNavJumpLoadsTail(input: {
  messageId: string
  lastItemMessageId: string | undefined
  hasMoreNewer: boolean
  hasDisconnectedLiveTail?: boolean
}): boolean {
  if (!input.lastItemMessageId) return false
  if (!input.hasMoreNewer && !input.hasDisconnectedLiveTail) return false
  return input.messageId === input.lastItemMessageId
}
