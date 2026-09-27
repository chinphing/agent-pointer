import type { ChatMessage, ContextCompressionInfo } from '../types/chat'
import { t } from '../i18n'

export const COMPRESSION_SUMMARY_PREFIX = '[Conversation summary (auto-compression'

/** Auto-compression summary chip (prefix user row, or in-run assistant row). */
export function isCompressionSummaryMessage(message: ChatMessage): boolean {
  return message.content.trimStart().startsWith(COMPRESSION_SUMMARY_PREFIX)
}

/** Prefix compression: user row before the next real question. */
export function isPrefixCompressionSummaryMessage(message: ChatMessage): boolean {
  return message.role === 'user' && isCompressionSummaryMessage(message)
}

/** In-run compression: assistant row mid-turn (process, not a turn header). */
export function isInRunCompressionSummaryMessage(message: ChatMessage): boolean {
  return message.role === 'assistant' && isCompressionSummaryMessage(message)
}

/** Strip the auto-compression header from a summary user row for display. */
export function compressionSummaryBody(content: string): string {
  const lines = content.split('\n')
  if (lines.length <= 1) return content.trim()
  const body = lines.slice(1).join('\n').trim()
  return body || content.trim()
}

export function buildCompressionNoticeContent(info: ContextCompressionInfo): string {
  const dropped = info.droppedCount
  if (info.scope === 'sub_agent') {
    const name = info.subAgentName?.trim() || t('chat.compression.subAgentFallback')
    return t('chat.compression.subAgentDone', { name, dropped })
  }
  if (info.reason === 'tool_limit' || info.reason === 'tool_limit_in_run') {
    return t('chat.compression.toolLimit', { dropped })
  }
  if (info.reason === 'in_run' || info.reason === 'overflow_in_run' || info.reason === 'in_run_drop') {
    return t('chat.compression.inRun', { dropped })
  }
  return t('chat.compression.default', { dropped })
}

/** In-thread tool-row label while compression LLM is running. */
export function buildCompressionProgressLabel(info: {
  scope?: string
  subAgentName?: string | null
  /** Marker already sits inside that sub-agent frame — same copy as the parent thread. */
  inSubAgentFrame?: boolean
}): string {
  if (info.scope === 'sub_agent' && !info.inSubAgentFrame) {
    const name = info.subAgentName?.trim() || t('chat.compression.subAgentFallback')
    return t('chat.compression.progressSub', { name })
  }
  return t('chat.compression.progress')
}

/** Parent turn list only places the lead-thread cut. Sub-agent cuts belong in SubAgentFrame. */
export function isParentThreadCompressionProgress<T extends { scope?: string }>(
  info: T | null | undefined
): info is T {
  return !!info && info.scope !== 'sub_agent'
}
