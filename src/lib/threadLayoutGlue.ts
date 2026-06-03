import type { ChatMessage } from '../types/chat'
import {
  assistantDisplayKind,
  isEphemeralDesktopNoticeMessage,
  isToolOnlyAssistantMessage
} from './assistantMessageKind'
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

/** Sidecar-only assistant round (verify_report, task_board, …) — no user-visible tool card. */
export function isSidecarOnlyAssistantMessage(message: ChatMessage): boolean {
  if (message.role !== 'assistant') return false
  const tcs = message.toolCalls ?? []
  if (tcs.length === 0) return false
  return tcs.every(tc => isSidecarToolCall(tc.name, toolCallBaseName(tc.name)))
}

/**
 * LLM wire rows that must not render as chat text (tool results, verify hints, sidecar rounds).
 * MessageRow already skips `role: tool`; glue layout must not resurrect them as plain text.
 */
export function isInternalThreadWireMessage(message: ChatMessage): boolean {
  if (message.role === 'tool') return true
  if (isEphemeralDesktopNoticeMessage(message)) return true
  if (message.role === 'assistant') {
    if (isSidecarOnlyAssistantMessage(message)) return true
    if (isToolOnlyAssistantMessage(message) && contentIsComputerVerifyHint(message.content ?? '')) {
      return true
    }
    if (contentIsComputerVerifyHint(message.content ?? '')) return true
  }
  return false
}

/** Screen inject only — compression / trim placeholders use normal message bubbles. */
export function isSyntheticThreadUserMessage(message: ChatMessage): boolean {
  return isScreenInjectUserMessage(message)
}

/** Real user task turn (not screen inject / compression / trim placeholder). */
export function isRealUserTaskMessage(message: ChatMessage): boolean {
  if (message.role !== 'user') return false
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
  if (isScreenInjectUserMessage(message)) return '桌面截图'
  const text = message.content?.trim() ?? ''
  if (!text) {
    if (message.role === 'tool') return '工具结果'
    return '…'
  }
  const oneLine = text.split('\n').find(l => l.trim())?.trim() ?? text
  if (oneLine.length <= 120) return oneLine
  return `${oneLine.slice(0, 117)}…`
}
