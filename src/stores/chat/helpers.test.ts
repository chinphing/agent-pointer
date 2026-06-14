import { describe, expect, it } from 'vitest'
import type { ChatMessage, Conversation } from '../../types/chat'
import {
  applyExcludedMessageIds,
  insertMessageBeforeAnchor,
  normalizeInterruptedAssistantStatuses,
  removeTrailingDiscardableEmptyAssistant
} from './helpers'

function conv(messages: ChatMessage[] = []): Conversation {
  return {
    id: 'c1',
    title: 't',
    messages,
    createdAt: 0,
    updatedAt: 0,
    skillIds: [],
    toolRoundsUsed: 0,
    toolRoundsUsedSupervisor: 0,
    leadAgentId: 'general',
    agentMode: 'single'
  }
}

describe('chat helpers', () => {
  it('applyExcludedMessageIds marks context state', () => {
    const c = conv([
      { id: 'm1', role: 'user', content: 'hi', status: 'done', createdAt: 0 }
    ])
    applyExcludedMessageIds(c, ['m1'], 'context_compression')
    expect(c.messages[0].contextState).toEqual({
      included: false,
      excludedReason: 'context_compression'
    })
  })

  it('insertMessageBeforeAnchor inserts before anchor id', () => {
    const c = conv([
      { id: 'u1', role: 'user', content: 'q', status: 'done', createdAt: 0 },
      { id: 'a1', role: 'assistant', content: '', status: 'done', createdAt: 0, toolCalls: [] }
    ])
    insertMessageBeforeAnchor(c, 'a1', {
      id: 'sum',
      role: 'assistant',
      content: 'summary',
      status: 'done',
      createdAt: 0,
      toolCalls: []
    })
    expect(c.messages.map(m => m.id)).toEqual(['u1', 'sum', 'a1'])
  })

  it('insertMessageBeforeAnchor skips duplicate id', () => {
    const c = conv([
      { id: 'u1', role: 'user', content: 'q', status: 'done', createdAt: 0 }
    ])
    insertMessageBeforeAnchor(c, 'u1', {
      id: 'u1',
      role: 'user',
      content: 'dup',
      status: 'done',
      createdAt: 0
    })
    expect(c.messages).toHaveLength(1)
  })

  it('normalizeInterruptedAssistantStatuses clears streaming flags', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: '',
        status: 'streaming',
        contentStreaming: true,
        createdAt: 0,
        toolCalls: []
      }
    ])
    normalizeInterruptedAssistantStatuses([c])
    expect(c.messages[0].status).toBe('done')
    expect(c.messages[0].contentStreaming).toBe(false)
  })

  it('removeTrailingDiscardableEmptyAssistant removes empty tail', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: '',
        status: 'done',
        contentStreaming: true,
        createdAt: 0,
        toolCalls: []
      }
    ])
    expect(removeTrailingDiscardableEmptyAssistant(c)).toBe(true)
    expect(c.messages).toHaveLength(0)
  })
})
