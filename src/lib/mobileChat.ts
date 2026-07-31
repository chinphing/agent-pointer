/** Mobile UI creates a fresh conversation only after more than 30 model invocations. */
export const MOBILE_NEW_CONVERSATION_TURN_THRESHOLD = 30

import type { ChatMessage } from '../types/chat'
import { assistantDisplayKind } from './assistantMessageKind'

/**
 * Count persisted assistant model rounds rather than user messages.
 * Each provider round is represented by an assistant message; injected notices
 * and non-model/error rows are excluded. Scoped assistant rows are included
 * because they are also real LLM invocations.
 */
export function countLlmInvocationRounds(messages: readonly ChatMessage[]): number {
  return messages.filter(message =>
    message.role === 'assistant' && assistantDisplayKind(message) === 'model'
  ).length
}

export function shouldShowMobileNewConversationButton(
  isMobileViewport: boolean,
  llmRoundCount: number
): boolean {
  return isMobileViewport && llmRoundCount > MOBILE_NEW_CONVERSATION_TURN_THRESHOLD
}
