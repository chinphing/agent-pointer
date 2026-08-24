import { isDiscardableEmptyAssistant } from '../../lib/assistantMessageKind'
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

/**
 * Index to splice a compression summary. Use the recorded keep-window id as-is
 * (including tool / glue rows). Walking to the next visible bubble would park
 * the chip on the final reply. Missing id → first non-excluded row, else append.
 */
export function resolveCompressionInsertAt(
  messages: readonly ChatMessage[],
  insertBeforeMessageId: string,
  excludedMessageIds: readonly string[] = []
): number {
  const anchor = insertBeforeMessageId.trim()
  if (anchor) {
    const idx = messages.findIndex(m => m.id === anchor)
    if (idx >= 0) return idx
  }

  const excluded = new Set(
    excludedMessageIds.map(id => id.trim()).filter(id => id.length > 0)
  )
  if (excluded.size > 0) {
    const firstKept = messages.findIndex(m => !excluded.has(m.id))
    if (firstKept >= 0) return firstKept
  }

  console.warn('[chat] compression summary insert: no keep anchor in thread', {
    insertBeforeMessageId: anchor || null,
    excluded: excluded.size,
    messages: messages.length
  })
  return messages.length
}

export function insertMessageBeforeAnchor(
  conv: Conversation,
  insertBeforeMessageId: string,
  message: ChatMessage,
  excludedMessageIds: readonly string[] = []
) {
  if (conv.messages.some(m => m.id === message.id)) return
  const insertAt = resolveCompressionInsertAt(
    conv.messages,
    insertBeforeMessageId,
    excludedMessageIds
  )
  conv.messages.splice(insertAt, 0, message)
}

export function removeAssistantMessage(conv: Conversation, messageId: string): boolean {
  const idx = conv.messages.findIndex(m => m.id === messageId)
  if (idx < 0) return false
  conv.messages.splice(idx, 1)
  conv.updatedAt = Date.now()
  return true
}

/** Transcript order: SQLite `position`, then `createdAt`, then id. Missing position = in-flight → after persisted rows. */
export function compareMessagesTranscriptOrder(a: ChatMessage, b: ChatMessage): number {
  const aPos = a.position
  const bPos = b.position
  const aMissing = aPos == null
  const bMissing = bPos == null
  if (aMissing !== bMissing) return aMissing ? 1 : -1
  if (!aMissing && !bMissing && aPos !== bPos) return aPos - bPos
  const aAt = a.createdAt ?? 0
  const bAt = b.createdAt ?? 0
  if (aAt !== bAt) return aAt - bAt
  return a.id.localeCompare(b.id)
}

export function sortMessagesInTranscriptOrder(messages: ChatMessage[]): ChatMessage[] {
  if (messages.length < 2) return messages
  return messages.slice().sort(compareMessagesTranscriptOrder)
}

function finiteCreatedAt(messages: readonly ChatMessage[]): number[] {
  return messages
    .map(m => m.createdAt)
    .filter((t): t is number => typeof t === 'number' && Number.isFinite(t) && t > 0)
}

function finitePositions(messages: readonly ChatMessage[]): number[] {
  return messages
    .map(m => m.position)
    .filter((p): p is number => typeof p === 'number' && Number.isFinite(p))
}

/**
 * Keep only rows that belong after the current window.
 * Bottom prefetch must never pull earlier turns (low createdAt / low position).
 */
export function retainIncomingNewerMessages(
  existing: readonly ChatMessage[],
  incoming: ChatMessage[]
): ChatMessage[] {
  if (incoming.length === 0) return incoming
  const existingPos = finitePositions(existing)
  const existingTimes = finiteCreatedAt(existing)
  const maxPos = existingPos.length ? Math.max(...existingPos) : null
  const minTime = existingTimes.length ? Math.min(...existingTimes) : null
  return incoming.filter(m => {
    if (minTime != null && m.createdAt != null && m.createdAt > 0 && m.createdAt < minTime) {
      return false
    }
    if (maxPos != null && m.position != null && m.position <= maxPos) {
      return false
    }
    return true
  })
}

/**
 * Overlay live streaming rows onto a DB page. Rows only in memory (typically
 * older turns already loaded) must not be appended after the tail window.
 */
export function mergeHydratedMessages(inMemory: ChatMessage[], fromDb: ChatMessage[]): ChatMessage[] {
  if (inMemory.length === 0) return fromDb
  const dbById = new Map(fromDb.map(m => [m.id, m]))
  const longer = (a?: string, b?: string) =>
    (a?.length ?? 0) >= (b?.length ?? 0) ? a : b
  const merged: ChatMessage[] = []
  for (const dbMsg of fromDb) {
    const live = inMemory.find(m => m.id === dbMsg.id)
    if (
      live
      && (live.status === 'streaming'
        || live.status === 'pending'
        || live.contentStreaming)
    ) {
      merged.push({
        ...dbMsg,
        ...live,
        content: longer(live.content, dbMsg.content) ?? '',
        reasoning: longer(live.reasoning, dbMsg.reasoning),
        rawContent: longer(live.rawContent, dbMsg.rawContent),
        thoughts: longer(live.thoughts, dbMsg.thoughts),
        toolCalls:
          (live.toolCalls?.length ?? 0) >= (dbMsg.toolCalls?.length ?? 0)
            ? live.toolCalls
            : dbMsg.toolCalls,
        attachments:
          (live.attachments?.length ?? 0) >= (dbMsg.attachments?.length ?? 0)
            ? live.attachments
            : dbMsg.attachments,
        agentTrace:
          (live.agentTrace?.length ?? 0) >= (dbMsg.agentTrace?.length ?? 0)
            ? live.agentTrace
            : dbMsg.agentTrace
      })
    } else {
      merged.push(dbMsg)
    }
  }
  for (const live of inMemory) {
    if (!dbById.has(live.id)) merged.push(live)
  }
  return sortMessagesInTranscriptOrder(merged)
}

export function mergeMessagePage(
  existing: ChatMessage[],
  incoming: ChatMessage[],
  direction: 'older' | 'newer'
): ChatMessage[] {
  if (incoming.length === 0) return existing
  const seen = new Set(existing.map(m => m.id))
  const unique = incoming.filter(m => {
    if (seen.has(m.id)) return false
    seen.add(m.id)
    return true
  })
  if (unique.length === 0) return existing
  if (direction === 'newer') {
    const retained = retainIncomingNewerMessages(existing, unique)
    if (retained.length === 0) {
      console.info(
        '[chat] mergeMessagePage: dropped older page requested as newer; bottom prefetch must not pull earlier turns'
      )
      return existing
    }
    return sortMessagesInTranscriptOrder([...existing, ...retained])
  }
  return sortMessagesInTranscriptOrder([...unique, ...existing])
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
 * before the first user message that should stay: never stamped into the
 * viewport/load map, or viewed within `staleMs`. Rows with a viewedAt older
 * than `staleMs` are eligible to drop. If every stamped user message is
 * stale (no keep pivot), time-based cut allows the whole list — the
 * `minKeepUserTurns` floor still caps how much is removed.
 *
 * Streamed rows without a `position` are never cut, and the new window head
 * is guaranteed to carry a position so the paging cursor stays exact.
 *
 * A floor of `minKeepUserTurns` user messages (default 8 = 1 page × 8 turns)
 * is always kept in memory: time-based trimming never removes more than that,
 * so a freshly-loaded thread is never thinned out below one paging page.
 */
export function computeHistoryTrimCutByViewedAt(
  messages: ChatMessage[],
  viewedAt: ReadonlyMap<string, number>,
  now: number,
  staleMs: number,
  minKeepUserTurns = 8
): number {
  if (messages.length === 0) return 0
  // Time-based scan: cut before the first user message that should stay.
  // Missing stamp → keep (just-loaded race before stamp applies).
  // All stamped + all stale → no pivot; allow cutting the whole list
  // (floor below still keeps the newest N user turns).
  let cutByTime = messages.length
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
