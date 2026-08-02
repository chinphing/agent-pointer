import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import {
  countLlmInvocationRounds,
  MOBILE_NEW_CONVERSATION_TURN_THRESHOLD,
  shouldShowFooterComposer,
  shouldShowMobileNewConversationButton
} from './mobileChat'

function assistant(id: string, content = 'reply'): ChatMessage {
  return {
    id,
    role: 'assistant',
    content,
    status: 'done',
    createdAt: 0
  }
}

describe('mobile new conversation entry', () => {
  it('counts assistant model rounds instead of user messages', () => {
    const messages: ChatMessage[] = [
      { id: 'u1', role: 'user', content: 'one', status: 'done', createdAt: 0 },
      assistant('a1'),
      { id: 'u2', role: 'user', content: 'two', status: 'done', createdAt: 0 },
      assistant('a2'),
      assistant('notice', '【提示】工具轮次已结束')
    ]

    expect(countLlmInvocationRounds(messages)).toBe(2)
  })

  it('appears only on mobile after more than 30 LLM rounds', () => {
    expect(
      shouldShowMobileNewConversationButton(true, MOBILE_NEW_CONVERSATION_TURN_THRESHOLD)
    ).toBe(false)
    expect(
      shouldShowMobileNewConversationButton(true, MOBILE_NEW_CONVERSATION_TURN_THRESHOLD + 1)
    ).toBe(true)
    expect(
      shouldShowMobileNewConversationButton(false, MOBILE_NEW_CONVERSATION_TURN_THRESHOLD + 1)
    ).toBe(false)
  })
})

describe('mobile welcome footer composer', () => {
  it('uses the bottom footer on mobile welcome, inline hero on desktop welcome', () => {
    expect(shouldShowFooterComposer(true, true, false)).toBe(true)
    expect(shouldShowFooterComposer(true, false, false)).toBe(false)
    expect(shouldShowFooterComposer(false, true, false)).toBe(true)
    expect(shouldShowFooterComposer(false, false, false)).toBe(true)
  })

  it('hides the footer while messages are hydrating', () => {
    expect(shouldShowFooterComposer(true, true, true)).toBe(false)
    expect(shouldShowFooterComposer(false, false, true)).toBe(false)
  })
})
