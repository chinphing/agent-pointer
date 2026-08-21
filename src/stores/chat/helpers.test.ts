import { describe, expect, it } from 'vitest'
import type { ChatMessage, Conversation } from '../../types/chat'
import {
  applyExcludedMessageIds,
  assistantTurnActivelyRunning,
  computeHistoryTrimCutByViewedAt,
  insertMessageBeforeAnchor,
  normalizeInterruptedAssistantStatuses,
  normalizeStaleEndedAssistantTurn,
  removeTrailingDiscardableEmptyAssistant,
  uid
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
  it('uid returns a UUID v4 string', () => {
    const id = uid()
    expect(id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
    )
  })

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

  it('insertMessageBeforeAnchor uses first kept visible row when keep id is missing', () => {
    const c = conv([
      { id: 'u1', role: 'user', content: 'old', status: 'done', createdAt: 0 },
      { id: 'a1', role: 'assistant', content: 'done', status: 'done', createdAt: 0, toolCalls: [] },
      { id: 'u2', role: 'user', content: 'keep', status: 'done', createdAt: 1 }
    ])
    insertMessageBeforeAnchor(
      c,
      'tool-not-in-ui',
      {
        id: 'sum',
        role: 'user',
        content: '[Conversation summary (auto-compression)]\nbody',
        status: 'done',
        createdAt: 0
      },
      ['u1', 'a1', 'tool-not-in-ui']
    )
    expect(c.messages.map(m => m.id)).toEqual(['u1', 'a1', 'sum', 'u2'])
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
          { id: 't1', name: 'shell', arguments: '{}', status: 'running' }
        ]
      }
    ])
    normalizeInterruptedAssistantStatuses([c])
    expect(c.messages[0].toolCalls![0].status).toBe('failed')
    expect(c.messages[0].toolCalls![0].error).toBe('interrupted')
  })

  it('normalizeInterruptedAssistantStatuses finalizes stuck agent traces', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: 'done reply',
        status: 'done',
        createdAt: 0,
        toolCalls: [],
        agentTrace: [
          {
            id: 'sub-1',
            name: 'explore',
            role: 'sub',
            status: 'running',
            depth: 1,
            session: {
              contentStreaming: true,
              collapsed: true,
              userExpanded: false,
              stats: { searchCount: 0, readCount: 0 },
              toolCalls: [{ id: 't1', name: 'shell', arguments: '{}', status: 'running' }]
            }
          }
        ]
      }
    ])
    normalizeInterruptedAssistantStatuses([c])
    expect(c.messages[0].agentTrace![0].status).toBe('completed')
    expect(c.messages[0].agentTrace![0].session!.contentStreaming).toBe(false)
    expect(c.messages[0].agentTrace![0].session!.toolCalls![0].status).toBe('failed')
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
        { id: 't1', name: 'grep', arguments: '{}', status: 'pending' }
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

  it('computeHistoryTrimCutByViewedAt keeps recently viewed user messages', () => {
    const now = 1_000_000
    const staleMs = 3_600_000 // 1h
    const msgs: ChatMessage[] = [
      { id: 'm0', role: 'user', content: 'a', status: 'done', createdAt: 0, position: 0 },
      { id: 'm1', role: 'assistant', content: 'b', status: 'done', createdAt: 0, position: 1 },
      { id: 'm2', role: 'user', content: 'c', status: 'done', createdAt: 0, position: 2 },
      { id: 'm3', role: 'assistant', content: 'd', status: 'done', createdAt: 0, position: 3 },
      // streamed row — no position, must never be cut
      { id: 'm4', role: 'assistant', content: 'e', status: 'streaming', createdAt: 0 }
    ]
    // Nothing viewed yet → keep everything (avoids wiping an unviewed thread).
    expect(computeHistoryTrimCutByViewedAt(msgs, new Map(), now, staleMs, 1)).toBe(0)
    // m0 never stamped, m2 recent → keep from m0 (do not treat missing as epoch 0).
    expect(
      computeHistoryTrimCutByViewedAt(
        msgs,
        new Map([['m2', now - 60_000]]),
        now,
        staleMs,
        1
      )
    ).toBe(0)
    // m0 viewed long ago, m2 recent → cut before m2.
    const viewed = new Map<string, number>([
      ['m0', now - 2 * staleMs],
      ['m2', now - 60_000]
    ])
    expect(computeHistoryTrimCutByViewedAt(msgs, viewed, now, staleMs, 1)).toBe(2)
    // All stamped users stale → no "recent" pivot; cut down to the keep floor
    // (newest 1 user turn → start at m2). Streamed m4 has no position so the
    // cut head must stay on a positioned row (still m2).
    const allStale = new Map<string, number>([
      ['m0', now - 2 * staleMs],
      ['m2', now - 2 * staleMs]
    ])
    expect(computeHistoryTrimCutByViewedAt(msgs, allStale, now, staleMs, 1)).toBe(2)
    // Freshly-viewed first user message → nothing is stale.
    const freshHead = new Map<string, number>([['m0', now - 1_000]])
    expect(computeHistoryTrimCutByViewedAt(msgs, freshHead, now, staleMs, 1)).toBe(0)
  })

  it('computeHistoryTrimCutByViewedAt all-stale trims to floor only', () => {
    const now = 1_000_000
    const staleMs = 3_600_000
    const user = (i: number): ChatMessage => ({
      id: `u${i}`,
      role: 'user',
      content: `q${i}`,
      status: 'done',
      createdAt: 0,
      position: i
    })
    const msgs: ChatMessage[] = Array.from({ length: 30 }, (_, i) => user(i))
    const viewed = new Map(msgs.map(m => [m.id, now - 2 * staleMs]))
    // Every stamp stale → time allows full cut; floor keeps newest 8 (u22..u29).
    expect(computeHistoryTrimCutByViewedAt(msgs, viewed, now, staleMs)).toBe(22)
  })

  it('computeHistoryTrimCutByViewedAt never trims below the keep floor', () => {
    const now = 1_000_000
    const staleMs = 3_600_000 // 1h
    const user = (i: number): ChatMessage => ({
      id: `u${i}`,
      role: 'user',
      content: `q${i}`,
      status: 'done',
      createdAt: 0,
      position: i
    })
    // 30 user messages, newest last.
    const msgs: ChatMessage[] = Array.from({ length: 30 }, (_, i) => user(i))
    // Only the newest 2 user messages were viewed recently.
    const viewed = new Map<string, number>()
    for (let i = 0; i < msgs.length; i += 1) {
      viewed.set(`u${i}`, i >= 28 ? now - 60_000 : now - 2 * staleMs)
    }
    // Default floor 8 → keep u22..u29 (8 turns) even though 28 are stale.
    expect(computeHistoryTrimCutByViewedAt(msgs, viewed, now, staleMs)).toBe(22)
    // Higher floor → even less trimming.
    expect(computeHistoryTrimCutByViewedAt(msgs, viewed, now, staleMs, 26)).toBe(4)
    // Fewer turns than the floor → never trim at all.
    const few: ChatMessage[] = Array.from({ length: 6 }, (_, i) => user(i))
    const fewViewed = new Map(few.map(m => [m.id, now - 2 * staleMs]))
    expect(computeHistoryTrimCutByViewedAt(few, fewViewed, now, staleMs)).toBe(0)
  })
})
