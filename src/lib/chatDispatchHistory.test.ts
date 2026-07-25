import { describe, expect, it } from 'vitest'
import {
  conversationNeedsHydration,
  messagesForChatDispatch
} from './chatDispatchHistory'
import type { ChatMessage } from '../types/chat'

describe('messagesForChatDispatch', () => {
  it('drops queued user messages and streaming assistant rows', () => {
    const messages: ChatMessage[] = [
      { id: 'u1', role: 'user', content: 'a', status: 'done', createdAt: 1 },
      { id: 'a1', role: 'assistant', content: 'partial', status: 'streaming', createdAt: 2 },
      { id: 'u2', role: 'user', content: 'queued', status: 'pending', createdAt: 3 }
    ]
    const out = messagesForChatDispatch(messages)
    expect(out.map(m => m.id)).toEqual(['u1'])
  })
})

describe('conversationNeedsHydration', () => {
  it('blocks a persisted shell even when partial in-memory rows exist', () => {
    expect(
      conversationNeedsHydration({
        messageCount: 20,
        messagesLength: 1,
        hydrated: false,
        loading: false
      })
    ).toBe(true)
  })

  it('allows new and fully hydrated conversations', () => {
    expect(
      conversationNeedsHydration({
        messageCount: 0,
        messagesLength: 1,
        hydrated: false,
        loading: false
      })
    ).toBe(false)
    expect(
      conversationNeedsHydration({
        messageCount: 20,
        messagesLength: 20,
        hydrated: true,
        loading: false
      })
    ).toBe(false)
  })
})
