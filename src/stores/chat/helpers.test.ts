import { describe, expect, it } from 'vitest'
import type { ChatMessage, Conversation } from '../../types/chat'
import {
  applyExcludedMessageIds,
  assistantTurnActivelyRunning,
  insertMessageBeforeAnchor,
  normalizeInterruptedAssistantStatuses,
  normalizeStaleEndedAssistantTurn,
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

  it('normalizeInterruptedAssistantStatuses finalizes stuck tool calls', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: 'done reply',
        status: 'done',
        createdAt: 0,
        toolCalls: [
          { id: 't1', name: 'shell', status: 'running', args: {}, createdAt: 0 }
        ]
      }
    ])
    normalizeInterruptedAssistantStatuses([c])
    expect(c.messages[0].toolCalls![0].status).toBe('failed')
    expect(c.messages[0].toolCalls![0].error).toBe('interrupted')
  })

  it('normalizeStaleEndedAssistantTurn clears stale streaming without active stream', () => {
    const msg: ChatMessage = {
      id: 'a1',
      role: 'assistant',
      content: 'finished reply',
      status: 'streaming',
      contentStreaming: false,
      createdAt: 0,
      toolCalls: []
    }
    normalizeStaleEndedAssistantTurn(msg)
    expect(msg.status).toBe('done')
    expect(assistantTurnActivelyRunning(msg)).toBe(false)
  })

  it('assistantTurnActivelyRunning stays true while content streams', () => {
    const msg: ChatMessage = {
      id: 'a1',
      role: 'assistant',
      content: 'partial',
      status: 'streaming',
      contentStreaming: true,
      createdAt: 0,
      toolCalls: []
    }
    expect(assistantTurnActivelyRunning(msg)).toBe(true)
    normalizeStaleEndedAssistantTurn(msg)
    expect(assistantTurnActivelyRunning(msg)).toBe(true)
  })

  it('normalizeStaleEndedAssistantTurn clears stuck tools on done assistant', () => {
    const msg: ChatMessage = {
      id: 'a1',
      role: 'assistant',
      content: 'reply',
      status: 'done',
      createdAt: 0,
      toolCalls: [
        { id: 't1', name: 'grep', status: 'pending', args: {}, createdAt: 0 }
      ]
    }
    normalizeStaleEndedAssistantTurn(msg)
    expect(msg.toolCalls![0].status).toBe('failed')
    expect(assistantTurnActivelyRunning(msg)).toBe(false)
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
