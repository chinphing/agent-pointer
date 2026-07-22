import type { ChatMessage, MessageStatus } from '../types/chat'
import { extractOutboundMediaPaths } from './outboundMedia'

/** User-visible assistant body: text and/or inline media (persisted attachments or `MEDIA:` draft). */
export function assistantHasDeliverableContent(message: ChatMessage): boolean {
  if (message.content?.trim()) return true
  if ((message.attachments?.length ?? 0) > 0) return true
  const raw = message.rawContent?.trim() ?? ''
  if (raw && extractOutboundMediaPaths(raw).length > 0) return true
  return false
}

/** How we render an assistant row in the thread (layout + emphasis). */
export type AssistantDisplayKind = 'model' | 'injected_notice' | 'error' | 'cancelled'

/** 全角 closing bracket `】`，勿写成 ASCII `]`（否则会匹配失败，提示行会走普通助手大气泡）。 */
const NOTICE_PREFIX_RE = /^【(桌面|提示|压缩)】/

export function assistantDisplayKind(message: ChatMessage): AssistantDisplayKind {
  if (message.role !== 'assistant') return 'model'
  if (NOTICE_PREFIX_RE.test(message.content.trim())) return 'injected_notice'

  const cancelled =
    message.status === 'cancelled' ||
    (message.status === 'error' && isGenerationCancelledMessage(message.errorMessage ?? ''))

  if (cancelled) {
    // Empty cancel → muted notice only. With tools/body → model row + muted footer.
    if (!assistantHasVisibleProgress(message)) return 'cancelled'
    return 'model'
  }

  if (message.status === 'error' && !message.content.trim()) return 'error'
  return 'model'
}

/** True when the assistant already showed tools, text, or reasoning before stop. */
export function assistantHasVisibleProgress(message: ChatMessage): boolean {
  if (assistantHasDeliverableContent(message)) return true
  if (message.thoughts?.trim()) return true
  if (message.reasoning?.trim()) return true
  if (message.responseTextDraft?.trim()) return true
  if (
    message.rawContent?.trim() &&
    message.rawContent !== message.content &&
    extractOutboundMediaPaths(message.rawContent).length === 0
  ) {
    return true
  }
  if ((message.toolCalls?.length ?? 0) > 0) return true
  if ((message.agentTrace?.length ?? 0) > 0) return true
  if ((message.supervisorPlanTasks?.length ?? 0) > 0) return true
  if (message.computerRoundScreenRelPath) return true
  return false
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

/** User cancelled generation (UI stop or host cancel). */
export function isGenerationCancelledMessage(message: string): boolean {
  const t = message.trim().toLowerCase()
  if (!t) return false
  if (t.includes('已停止')) return true
  // Provider / Tokio cancel paths often surface bare English "cancelled".
  if (/\bcancell?ed\b/.test(t)) return true
  return false
}

/** Empty assistant shell that can be dropped when the user stops generation. */
export function isDiscardableEmptyAssistant(message: ChatMessage): boolean {
  if (message.role !== 'assistant') return false
  if (message.status === 'error' || message.status === 'cancelled') return false
  if (isEphemeralDesktopNoticeMessage(message)) return false
  if (NOTICE_PREFIX_RE.test(message.content.trim())) return false
  // Keep the active shell visible while streaming so「思考中…」can render before first delta.
  if (isMessageStreaming(message.status)) return false

  return !assistantHasVisibleProgress(message)
}

/**
 * Like {@link isDiscardableEmptyAssistant}, but for the stop/cancel path:
 * streaming empty shells should also be removed (no red error card).
 */
export function isDiscardableEmptyAssistantOnCancel(message: ChatMessage): boolean {
  if (message.role !== 'assistant') return false
  if (isEphemeralDesktopNoticeMessage(message)) return false
  if (NOTICE_PREFIX_RE.test(message.content.trim())) return false
  return !assistantHasVisibleProgress(message)
}

/** Assistant row that only shows tool calls (no user-visible reply body). */
export function isToolOnlyAssistantMessage(message: ChatMessage): boolean {
  if (message.role !== 'assistant') return false
  if (isDiscardableEmptyAssistant(message)) return false
  if (assistantDisplayKind(message) !== 'model') return false
  if ((message.toolCalls?.length ?? 0) === 0) return false

  // Internal wire — not user-visible in the thread (see isToolOnlyAssistantMessage).
  const hasVisibleText =
    !!(message.content?.trim()) ||
    !!(message.responseTextDraft?.trim())

  const hasOtherStructure =
    (message.agentTrace?.length ?? 0) > 0 ||
    (message.supervisorPlanTasks?.length ?? 0) > 0

  return !hasVisibleText && !hasOtherStructure
}
