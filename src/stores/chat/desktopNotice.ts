import type { Ref } from 'vue'
import type { Conversation } from '../../types/chat'
import { isEphemeralDesktopNoticeMessage } from '../../lib/assistantMessageKind'

const DESKTOP_NOTICE_HIDE_MS = 5000

export interface DesktopNoticeScheduler {
  scheduleRemoval(conversationId: string, messageId: string): void
  clearSchedule(messageId: string): void
}

export function createDesktopNoticeScheduler(deps: {
  conversations: Ref<Conversation[]>
  hideMs?: number
}): DesktopNoticeScheduler {
  const hideTimers = new Map<string, number>()
  const hideMs = deps.hideMs ?? DESKTOP_NOTICE_HIDE_MS

  function clearSchedule(messageId: string) {
    const t = hideTimers.get(messageId)
    if (t != null) {
      window.clearTimeout(t)
      hideTimers.delete(messageId)
    }
  }

  function removeRow(conversationId: string, messageId: string) {
    const conv = deps.conversations.value.find(c => c.id === conversationId)
    if (!conv) return
    const i = conv.messages.findIndex(m => m.id === messageId)
    if (i < 0) return
    const msg = conv.messages[i]
    if (!isEphemeralDesktopNoticeMessage(msg)) return
    conv.messages.splice(i, 1)
    conv.updatedAt = Date.now()
  }

  function scheduleRemoval(conversationId: string, messageId: string) {
    clearSchedule(messageId)
    hideTimers.set(
      messageId,
      window.setTimeout(() => {
        hideTimers.delete(messageId)
        removeRow(conversationId, messageId)
      }, hideMs)
    )
  }

  return { scheduleRemoval, clearSchedule }
}
