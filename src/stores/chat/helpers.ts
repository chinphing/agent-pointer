import { isDiscardableEmptyAssistant } from '../../lib/assistantMessageKind'
import type { ChatMessage, Conversation, ExcludedReason } from '../../types/chat'

/** Conversation / message client ids — UUID v4 (stable opaque segment for media paths). */
export function uid() {
  return crypto.randomUUID()
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

function finalizeStuckToolCalls(msg: ChatMessage): void {
  for (const tc of msg.toolCalls ?? []) {
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

/** True only when the assistant turn is still receiving stream events or running tools. */
export function assistantTurnActivelyRunning(msg: ChatMessage): boolean {
  if (msg.role !== 'assistant') return false
  if (msg.status !== 'streaming' && msg.status !== 'pending') return false
  return msg.contentStreaming === true || hasInFlightToolCalls(msg)
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
    }
  }
}
