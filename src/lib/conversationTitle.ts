import { t } from '../i18n'
import { imConversationTitle, isImConversation } from './channel-labels'
import { isRealUserTaskMessage } from './threadLayoutGlue'
import type { Conversation } from '../types/chat'

/** Legacy / locale variants of the empty-conversation sidebar title. */
const DEFAULT_TITLE_ALIASES = new Set(['新会话', 'New chat'])

/** Locale-aware default title for new conversations. */
export function defaultConversationTitle(): string {
  return t('chat.newConversation')
}

/**
 * @deprecated Prefer `defaultConversationTitle()` / `isDefaultConversationTitle()`.
 * Kept as a zh-CN sentinel so older comparisons and tests keep working.
 */
export const DEFAULT_CONVERSATION_TITLE = '新会话'

export function isDefaultConversationTitle(title: string | null | undefined): boolean {
  const v = (title ?? '').trim()
  if (!v) return true
  if (DEFAULT_TITLE_ALIASES.has(v)) return true
  return v === t('chat.newConversation')
}

/** Sidebar title from first user message when still on the default placeholder. */
export function deriveConversationTitle(conv: Conversation): string | null {
  if (!isDefaultConversationTitle(conv.title)) return null
  const firstUser = conv.messages.find(m => isRealUserTaskMessage(m) && m.content.trim())
  if (isImConversation(conv.id)) {
    return imConversationTitle(conv.id, { firstUserText: firstUser?.content })
  }
  if (firstUser) {
    const trimmed = firstUser.content.trim()
    return trimmed.slice(0, 24) || defaultConversationTitle()
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
