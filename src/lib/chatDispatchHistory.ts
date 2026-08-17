import type { ChatMessage } from '../types/chat'

export interface ConversationHydrationState {
  messageCount: number
  messagesLength: number
  hydrated: boolean
  loading: boolean
}

export interface MessagesForChatDispatchOptions {
  /**
   * Message ids already known to exist in the SQLite transcript.
   * When set, only non-persisted rows are cloned into the `sendChat` payload
   * (backend `append_missing` + DB reload remains the source of truth).
   */
  persistedIds?: ReadonlySet<string>
}

/** Whether sending must wait for the canonical persisted transcript. */
export function conversationNeedsHydration(state: ConversationHydrationState): boolean {
  if (state.loading) return true
  if (state.messageCount > 0 && !state.hydrated) return true
  return !state.hydrated && state.messagesLength === 0
}

function isDispatchableMessage(m: ChatMessage): boolean {
  if (m.status === 'pending') return false
  if (m.role === 'assistant' && m.status === 'streaming') return false
  return true
}

/** rawContent is UI-only debug data — never ship it to the backend (wire / DB). */
function cloneWithoutRawContent(m: ChatMessage): ChatMessage {
  const clone = JSON.parse(JSON.stringify(m)) as ChatMessage
  delete clone.rawContent
  return clone
}

/** Messages included in the next `sendChat` / dispatcher history snapshot. */
export function messagesForChatDispatch(
  messages: ChatMessage[],
  options?: MessagesForChatDispatchOptions
): ChatMessage[] {
  const persisted = options?.persistedIds
  return messages
    .filter(m => {
      if (!isDispatchableMessage(m)) return false
      if (persisted?.has(m.id)) return false
      return true
    })
    .map(cloneWithoutRawContent)
}

export interface MessagesForPersistAppendOptions {
  /**
   * Message ids already treated as on-disk for a hydrated conversation.
   * When set, only non-persisted rows are deep-cloned for `append_conversation_messages`.
   */
  persistedIds?: ReadonlySet<string>
}

/**
 * Messages to ship on `persistAppend` (Done / trim / send failure).
 * Callers should strip ephemeral desktop notices and wire-only attachment fields first.
 */
export function messagesForPersistAppend(
  messages: ChatMessage[],
  options?: MessagesForPersistAppendOptions
): ChatMessage[] {
  const persisted = options?.persistedIds
  return messages
    .filter(m => {
      if (m.status === 'pending') return false
      if (persisted?.has(m.id)) return false
      return true
    })
    .map(cloneWithoutRawContent)
}

/** Ids safe to treat as on-disk after hydration / append (excludes outbound queue rows). */
export function persistedCandidateMessageIds(messages: ChatMessage[]): string[] {
  return messages.filter(m => m.status !== 'pending').map(m => m.id)
}
