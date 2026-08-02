import { imConversationTitle, isImConversation } from './channel-labels'
import { isRealUserTaskMessage } from './threadLayoutGlue'
import type { Conversation } from '../types/chat'

export const DEFAULT_CONVERSATION_TITLE = '新会话'

/** Sidebar title from first user message when still on the default placeholder. */
export function deriveConversationTitle(conv: Conversation): string | null {
  if (conv.title !== DEFAULT_CONVERSATION_TITLE) return null
  const firstUser = conv.messages.find(m => isRealUserTaskMessage(m) && m.content.trim())
  if (isImConversation(conv.id)) {
    return imConversationTitle(conv.id, { firstUserText: firstUser?.content })
  }
  if (firstUser) {
    const trimmed = firstUser.content.trim()
    return trimmed.slice(0, 24) || DEFAULT_CONVERSATION_TITLE
  }
  return null
}

export function maybeUpdateConversationTitle(conv: Conversation): boolean {
  const next = deriveConversationTitle(conv)
  if (!next || next === conv.title) return false
  conv.title = next
  return true
}

export function normalizeDefaultConversationTitles(list: Conversation[]): boolean {
  let changed = false
  for (const conv of list) {
    if (maybeUpdateConversationTitle(conv)) changed = true
  }
  return changed
}
