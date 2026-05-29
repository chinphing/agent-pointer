import type { ChatMessage, ExcludedReason } from '../types/chat'

export function isContextIncluded(message: ChatMessage): boolean {
  return message.contextState?.included !== false
}

export function isContextExcluded(message: ChatMessage): boolean {
  return message.contextState?.included === false
}

export function excludedReasonLabel(reason: ExcludedReason | undefined): string {
  if (reason === 'context_compression') return '上下文压缩'
  if (reason === 'task_board_trim') return '任务板精简'
  return '未纳入上下文'
}

export function clearTaskBoardAnchors(messages: ChatMessage[]): void {
  for (const m of messages) {
    if (m.uiBindings?.taskBoardAnchor) {
      m.uiBindings = { ...m.uiBindings, taskBoardAnchor: false }
    }
  }
}

export function setTaskBoardAnchor(messages: ChatMessage[], messageId: string): void {
  clearTaskBoardAnchors(messages)
  const target = messages.find(m => m.id === messageId)
  if (!target) return
  target.uiBindings = { ...target.uiBindings, taskBoardAnchor: true }
}

export function findTaskBoardAnchorMessage(messages: ChatMessage[]): ChatMessage | null {
  return messages.find(m => m.uiBindings?.taskBoardAnchor === true) ?? null
}

export function findLastRealUserMessage(messages: ChatMessage[]): ChatMessage | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]
    if (m.role === 'user') return m
  }
  return null
}

/** One-time recovery when board data exists but no anchor was persisted. */
export function ensureTaskBoardAnchor(messages: ChatMessage[]): boolean {
  if (findTaskBoardAnchorMessage(messages)) return false
  const lastUser = findLastRealUserMessage(messages)
  if (!lastUser) return false
  setTaskBoardAnchor(messages, lastUser.id)
  return true
}
