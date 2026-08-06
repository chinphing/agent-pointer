import {
  isDiscardableEmptyAssistant
} from '../../lib/assistantMessageKind'
import { randomUuid } from '../../lib/randomUuid'
import type { ChatMessage, Conversation, ExcludedReason, ToolCall } from '../../types/chat'

/** Conversation / message client ids — UUID v4 (stable opaque segment for media paths). */
export function uid() {
  return randomUuid()
}

function excludedContextState(reason: ExcludedReason): ChatMessage['contextState'] {
  return { included: false, excludedReason: reason }
}

export function applyExcludedMessageIds(
  conv: Conversation,
  messageIds: string[],
  reason: ExcludedReason
) {
  const state = excludedContextState(reason)
  for (const id of messageIds) {
    const msg = conv.messages.find(m => m.id === id)
    if (msg) msg.contextState = state
  }
}

export function insertMessageBeforeAnchor(
  conv: Conversation,
  insertBeforeMessageId: string,
  message: ChatMessage
) {
  if (conv.messages.some(m => m.id === message.id)) return
  const anchor = insertBeforeMessageId.trim()
  const idx = anchor ? conv.messages.findIndex(m => m.id === anchor) : -1
  const insertAt = idx >= 0 ? idx : conv.messages.length
  conv.messages.splice(insertAt, 0, message)
}

export function removeAssistantMessage(conv: Conversation, messageId: string): boolean {
  const idx = conv.messages.findIndex(m => m.id === messageId)
  if (idx < 0) return false
  conv.messages.splice(idx, 1)
  conv.updatedAt = Date.now()
  return true
}

export function removeTrailingDiscardableEmptyAssistant(conv: Conversation): boolean {
  const last = conv.messages[conv.messages.length - 1]
  if (!last || !isDiscardableEmptyAssistant(last)) return false
  conv.messages.pop()
  conv.updatedAt = Date.now()
  return true
}

/**
 * After user stop without a messageId, keep the trailing assistant row and mark it
 * cancelled so the muted「已停止生成」caption remains visible.
 */
export function markTrailingAssistantCancelled(conv: Conversation): boolean {
  const last = conv.messages[conv.messages.length - 1]
  if (!last || last.role !== 'assistant') return false
  if (last.status === 'done' || last.status === 'error') return false
  last.status = 'cancelled'
  last.errorMessage = '已停止生成'
  last.contentStreaming = false
  conv.updatedAt = Date.now()
  return true
}

export function hasInFlightToolCalls(msg: ChatMessage): boolean {
  return (
    msg.toolCalls?.some(
      t =>
        t.status === 'running' ||
        t.status === 'pending' ||
        t.status === 'pending_approval'
    ) ?? false
  )
}

function finalizeStuckToolCallsList(toolCalls: ToolCall[] | undefined): void {
  for (const tc of toolCalls ?? []) {
    if (
      tc.status !== 'running' &&
      tc.status !== 'pending' &&
      tc.status !== 'pending_approval'
    ) {
      continue
    }
    if (tc.status === 'pending_approval') {
      tc.status = 'rejected'
      continue
    }
    tc.status = tc.result?.trim() ? 'success' : 'failed'
    if (tc.status === 'failed' && !tc.error) tc.error = 'interrupted'
  }
}

function finalizeStuckToolCalls(msg: ChatMessage): void {
  finalizeStuckToolCallsList(msg.toolCalls)
}

/** True only when the assistant turn is still receiving stream events or running tools. */
export function assistantTurnActivelyRunning(msg: ChatMessage): boolean {
  if (msg.role !== 'assistant') return false
  if (msg.status !== 'streaming' && msg.status !== 'pending') return false
  return msg.contentStreaming === true || hasInFlightToolCalls(msg)
}

/**
 * Number of leading messages that may be safely trimmed from an in-memory
 * conversation history, based on per-user-message last-viewed timestamps.
 *
 * Scans user messages from oldest to newest and returns the cut index just
 * before the first user message that should stay: either never stamped into
 * the viewport, or viewed within `staleMs`. Rows with a viewedAt older than
 * `staleMs` are eligible to drop. Streamed rows without a `position` are never
 * cut, and the new window head is guaranteed to carry a position so the paging
 * cursor stays exact.
 *
 * A floor of `minKeepUserTurns` user messages (default 24 = 3 pages × 8 turns)
 * is always kept in memory: time-based trimming never removes more than that,
 * so a freshly-loaded thread is never thinned out below three paging pages.
 */
export function computeHistoryTrimCutByViewedAt(
  messages: ChatMessage[],
  viewedAt: ReadonlyMap<string, number>,
  now: number,
  staleMs: number,
  minKeepUserTurns = 24
): number {
  if (messages.length === 0) return 0
  // Time-based scan: cut before the first user message that should stay in
  // memory. A message with no viewedAt stamp has never been in the viewport —
  // treat it like "keep" so a just-prepended older page is not immediately
  // deleted (viewedAt missing must not be coerced to epoch 0 / "ancient").
  let cutByTime = 0
  for (let i = 0; i < messages.length; i += 1) {
    const msg = messages[i]!
    if (msg.role !== 'user') continue
    const viewed = viewedAt.get(msg.id)
    if (viewed == null || now - viewed <= staleMs) {
      cutByTime = i
      break
    }
  }
  // Floor: keep at least `minKeepUserTurns` user turns. Counting from the
  // newest user message backward, the floor index is the start of the window
  // that still holds that many turns; fewer turns than the floor → 0 (no trim).
  let floor = 0
  let userCount = 0
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i]!.role === 'user') userCount += 1
    if (userCount === minKeepUserTurns) {
      floor = i
      break
    }
  }
  let cut = Math.min(cutByTime, floor)
  // A streamed / not-yet-persisted row may not have a position; never cut such
  // rows, and never leave a position-less row as the new window head.
  if (cut > 0 && messages[cut]?.position == null) {
    cut = Math.max(0, cut - 1)
  }
  return cut
}

/**
 * Clear stale `streaming`/`pending` flags and stuck tool rows on turns that
 * already finished (e.g. after reload or when revisiting an ended session).
 */
export function normalizeStaleEndedAssistantTurn(msg: ChatMessage): void {
  if (msg.role !== 'assistant') return
  const inFlight = hasInFlightToolCalls(msg)
  const looksEnded =
    msg.status === 'done' ||
    ((msg.status === 'streaming' || msg.status === 'pending') &&
      !msg.contentStreaming &&
      !inFlight)
  if (!looksEnded) return
  if (msg.status === 'streaming' || msg.status === 'pending') {
    msg.status = 'done'
  }
  msg.contentStreaming = false
  finalizeStuckToolCalls(msg)
}

function finalizeStuckAgentTraces(msg: ChatMessage): void {
  for (const trace of msg.agentTrace ?? []) {
    const st = (trace.status || '').trim()
    if (st === 'running' || st === 'streaming' || st === 'pending') {
      trace.status = 'completed'
    }
    if (trace.session) {
      trace.session.contentStreaming = false
      finalizeStuckToolCallsList(trace.session.toolCalls)
    }
  }
}

/** After reload or stop, assistant rows must not stay `streaming`/`pending`. */
export function normalizeInterruptedAssistantStatuses(conversations: Conversation[]): void {
  for (const conv of conversations) {
    for (const m of conv.messages) {
      if (m.role !== 'assistant') continue
      if (m.status === 'streaming' || m.status === 'pending') {
        m.status = 'done'
      }
      m.contentStreaming = false
      finalizeStuckToolCalls(m)
      finalizeStuckAgentTraces(m)
    }
  }
}
