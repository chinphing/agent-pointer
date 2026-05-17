import type { ChatMessage, MessageStatus } from '../types/chat'

/** How we render an assistant row in the thread (layout + emphasis). */
export type AssistantDisplayKind = 'model' | 'injected_notice' | 'error'

/** 全角 closing bracket `】`，勿写成 ASCII `]`（否则会匹配失败，提示行会走普通助手大气泡）。 */
const NOTICE_PREFIX_RE = /^【(桌面|提示|压缩)】/

export function assistantDisplayKind(message: ChatMessage): AssistantDisplayKind {
  if (message.role !== 'assistant') return 'model'
  if (NOTICE_PREFIX_RE.test(message.content.trim())) return 'injected_notice'
  if (message.status === 'error' && !message.content.trim()) return 'error'
  return 'model'
}

/** Sub-styles for injected lines (copy/tones only; layout stays `injected_notice`). */
export function injectedNoticeFlavor(content: string): 'desktop' | 'hint' | 'compression' | 'generic' {
  const t = content.trim()
  if (t.startsWith('【桌面】')) return 'desktop'
  if (t.startsWith('【提示】')) return 'hint'
  if (t.startsWith('【压缩】')) return 'compression'
  return 'generic'
}

/** 桌面截图流水线注入的助手状态行：仅会话内展示，不写本地会话存档。 */
export function isEphemeralDesktopNoticeMessage(message: ChatMessage): boolean {
  return message.role === 'assistant' && message.content.trimStart().startsWith('【桌面】')
}

export function isMessageStreaming(status: MessageStatus): boolean {
  return status === 'streaming' || status === 'pending'
}
