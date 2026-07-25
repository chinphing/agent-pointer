import type { ChatMessage } from '../types/chat'

export interface ConversationHydrationState {
  messageCount: number
  messagesLength: number
  hydrated: boolean
  loading: boolean
}

/** Whether sending must wait for the canonical persisted transcript. */
export function conversationNeedsHydration(state: ConversationHydrationState): boolean {
  if (state.loading) return true
  if (state.messageCount > 0 && !state.hydrated) return true
  return !state.hydrated && state.messagesLength === 0
}

/** Messages included in the next `sendChat` / dispatcher history snapshot. */
export function messagesForChatDispatch(messages: ChatMessage[]): ChatMessage[] {
  return messages
    .filter(m => {
      if (m.status === 'pending') return false
      if (m.role === 'assistant' && m.status === 'streaming') return false
      return true
    })
    .map(m => JSON.parse(JSON.stringify(m)) as ChatMessage)
}
