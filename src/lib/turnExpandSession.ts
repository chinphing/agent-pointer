/**
 * Expand / collapse persistence for one mounted `MessageList` instance.
 *
 * `ChatView` renders `MessageList` with `:key="chat.currentId"`, so an instance
 * lives for exactly one conversation. Capture that id when the instance is
 * created and use it for every read and write: by the time `onBeforeUnmount`
 * runs, the chat store already points at the conversation being opened, so
 * re-reading `chat.currentId` there saved the outgoing conversation's snapshot
 * into the incoming conversation's bucket (every switch copied the union of
 * everything seen so far into the next bucket).
 */

import {
  loadTurnExpandUiState,
  saveTurnExpandUiState,
  type TurnExpandUiState
} from './turnExpandState'

export type TurnExpandSession = {
  /** Conversation this instance was mounted for (`''` when the store had none). */
  readonly conversationId: string
  /** Overrides stored for this instance's conversation. */
  load(): ReturnType<typeof loadTurnExpandUiState>
  /** Write-through save; no-op when the instance was mounted without an id. */
  save(state: TurnExpandUiState): void
}

export function createTurnExpandSession(
  mountedConversationId: string | null | undefined
): TurnExpandSession {
  const conversationId = mountedConversationId?.trim() ?? ''
  return {
    conversationId,
    load: () => loadTurnExpandUiState(conversationId),
    save(state) {
      if (!conversationId) return
      saveTurnExpandUiState(conversationId, state)
    }
  }
}
