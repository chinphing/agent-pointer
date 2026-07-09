import type { ChatMessage } from '../types/chat'

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
