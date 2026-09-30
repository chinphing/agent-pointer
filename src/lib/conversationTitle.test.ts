import { describe, expect, it } from 'vitest'
import {
  DEFAULT_CONVERSATION_TITLE,
  deriveConversationTitle,
  maybeUpdateConversationTitle
} from './conversationTitle'
import type { Conversation } from '../types/chat'

function conv(partial: Partial<Conversation> & Pick<Conversation, 'id'>): Conversation {
  return {
    title: partial.title ?? DEFAULT_CONVERSATION_TITLE,
    messages: partial.messages ?? [],
    createdAt: 0,
    updatedAt: 0,
    skillIds: [],
    toolRoundsUsed: 0,
    leadAgentId: 'general',
    ...partial
  }
}

describe('conversationTitle', () => {
  it('derives desktop title from first user message', () => {
    const c = conv({
      id: 'c1',
      messages: [{ id: 'u1', role: 'user', content: '帮我打开 Chrome', status: 'done', createdAt: 1 }]
    })
    expect(deriveConversationTitle(c)).toBe('帮我打开 Chrome')
  })

  it('leaves custom title unchanged', () => {
    const c = conv({
      id: 'c1',
      title: '已有标题',
      messages: [{ id: 'u1', role: 'user', content: 'x', status: 'done', createdAt: 1 }]
    })
    expect(deriveConversationTitle(c)).toBeNull()
  })

  it('updates title in place', () => {
    const c = conv({
      id: 'c1',
      messages: [{ id: 'u1', role: 'user', content: 'hello world', status: 'done', createdAt: 1 }]
    })
    expect(maybeUpdateConversationTitle(c)).toBe(true)
    expect(c.title).toBe('hello world')
  })
})
