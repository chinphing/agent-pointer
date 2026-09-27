import { t } from '../i18n'
import type { ChatMessage } from '../types/chat'
import {
  assistantDisplayKind,
  isEphemeralDesktopNoticeMessage,
  isToolOnlyAssistantMessage
} from './assistantMessageKind'
import { isCompressionSummaryMessage } from './compressionMessage'
import { isSidecarToolCall, toolCallBaseName } from './messageTooling'

const VERIFY_HINT_PREFIXES = [
  'Sidecar verify signal accepted:',
  'Attempted overlay click',
  'Attempted click at coordinates',
  'Attempted click action',
  'Attempted scroll:',
  'Attempted text input action',
  'Attempted hotkey action',
  'Attempted action:',
  'Goal step complete: waited'
] as const

function contentIsComputerVerifyHint(text: string): boolean {
  const t = text.trim()
  return VERIFY_HINT_PREFIXES.some(p => t.startsWith(p))
}

export function isScreenInjectUserMessage(message: ChatMessage): boolean {
  if (message.role !== 'user') return false
  return message.content.trimStart().startsWith('[CUR_SCREEN]')
}

/**
 * Backend-injected format / empty-response / provider-recovery turns
 * (`push_injected_format_retry_turn`). Kept in model history; never show as
 * user chat bubbles. Aligns with `is_synthetic_user_content` retry prefixes
 * (not compression/trim placeholders, which still render).
 */
export function isInternalRetryUserMessage(message: ChatMessage): boolean {
  if (message.role !== 'user') return false
  if (message.id.startsWith('fmt_retry_')) return true
  const t = message.content.trimStart()
  return (
    t.startsWith('你的上一次回复为空') ||
    t.startsWith('Your previous reply was empty') ||
    t.startsWith('【环境反馈】') ||
    t.startsWith('【Environment feedback】') ||
    t.startsWith('【输出长度】') ||
    t.startsWith('【Output length】')
  )
}

/** Sidecar-only assistant round (task_board, …) — no user-visible tool card. */
export function isSidecarOnlyAssistantMessage(message: ChatMessage): boolean {
  if (message.role !== 'assistant') return false
  const tcs = message.toolCalls ?? []
  if (tcs.length === 0) return false
  return tcs.every(tc => isSidecarToolCall(tc.name, toolCallBaseName(tc.name)))
}

/**
 * LLM wire rows that must not render as chat text (tool results, verify hints, sidecar rounds).
 * MessageRow already skips `role: tool`; glue layout must not resurrect them as plain text.
 *
 * `【桌面】` capture status lines are **not** wire — they render via `AssistantNoticeMessage`
 * (`MessageList` normal `message` entries). Do not fold them into `isToolRunContinuityGlue`.
 */
export function isInternalThreadWireMessage(message: ChatMessage): boolean {
  if (message.role === 'tool') return true
  if (message.role === 'assistant') {
    if (isSidecarOnlyAssistantMessage(message)) return true
    if (isToolOnlyAssistantMessage(message) && contentIsComputerVerifyHint(message.content ?? '')) {
      return true
    }
    if (contentIsComputerVerifyHint(message.content ?? '')) return true
  }
  return false
}

/**
 * Synthetic user rows that must not render as chat text.
 * Compression / trim placeholders still use normal message bubbles.
 */
export function isSyntheticThreadUserMessage(message: ChatMessage): boolean {
  return isScreenInjectUserMessage(message) || isInternalRetryUserMessage(message)
}

/** Real user task turn (not screen inject / retry inject / compression chip). */
export function isRealUserTaskMessage(message: ChatMessage): boolean {
  if (message.role !== 'user') return false
  if (isCompressionSummaryMessage(message)) return false
  return !isSyntheticThreadUserMessage(message)
}

/** Assistant row with user-visible dialog (not tool-only / notice). */
export function isRealAssistantDialogMessage(message: ChatMessage): boolean {
  if (message.role !== 'assistant') return false
  if (isToolOnlyAssistantMessage(message)) return false
  if (isEphemeralDesktopNoticeMessage(message)) return false
  return assistantDisplayKind(message) === 'model'
}

/**
 * Keeps consecutive tool cards in one block without rendering a row
 * (tool results, sidecar rounds, synthetic injects).
 */
export function isSilentToolRunGlue(message: ChatMessage): boolean {
  if (isInternalThreadWireMessage(message)) return true
  if (isSyntheticThreadUserMessage(message)) return true
  return false
}

/** Compact inline row inside a tool run (screen inject only). */
export function shouldShowCompactGlueInToolRun(message: ChatMessage): boolean {
  return isScreenInjectUserMessage(message)
}

export function isToolRunContinuityGlue(message: ChatMessage): boolean {
  return isSilentToolRunGlue(message) || shouldShowCompactGlueInToolRun(message)
}

/** Glue rows in the thread (screen inject marker when debug UI is on). */
export function shouldShowGlueMessage(
  message: ChatMessage,
  settings?: { computerAnnotatedScreenViewEnabled?: boolean }
): boolean {
  if (!shouldShowCompactGlueInToolRun(message)) return false
  return settings?.computerAnnotatedScreenViewEnabled === true
}

export function glueMessagePreview(message: ChatMessage): string {
  if (isScreenInjectUserMessage(message)) return t('thread.desktopScreenshot')
  const text = message.content?.trim() ?? ''
  if (!text) {
    if (message.role === 'tool') return t('thread.toolResult')
    return '…'
  }
  const oneLine = text.split('\n').find(l => l.trim())?.trim() ?? text
  if (oneLine.length <= 120) return oneLine
  return `${oneLine.slice(0, 117)}…`
}
