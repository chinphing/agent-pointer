import { t } from '../i18n'
import { imConversationTitle, isImConversation } from './channel-labels'
import { isRealUserTaskMessage } from './threadLayoutGlue'
import type { Conversation } from '../types/chat'

export function defaultConversationTitle(): string {
  return t('chat.defaultTitle')
}

/** Legacy / bilingual placeholders still treated as “untitled”. */
const LEGACY_DEFAULT_TITLES = new Set(['新会话', '新建会话', 'New chat', 'New conversation'])

/** Snapshot at module load; prefer defaultConversationTitle() when locale may change. */
export const DEFAULT_CONVERSATION_TITLE = defaultConversationTitle()

export function isDefaultConversationTitle(title: string): boolean {
  return title === defaultConversationTitle() || LEGACY_DEFAULT_TITLES.has(title)
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
