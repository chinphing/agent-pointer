import { describe, expect, it } from 'vitest'
import { messagesForChatDispatch } from './chatDispatchHistory'
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
