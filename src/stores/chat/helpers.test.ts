import { describe, expect, it } from 'vitest'
import type { ChatMessage, Conversation } from '../../types/chat'
import {
  applyExcludedMessageIds,
  assistantTurnActivelyRunning,
  closeAbandonedEmptyAssistantShells,
  computeHistoryTrimCutByViewedAt,
  conversationNeedsTailReload,
  hasDisconnectedLiveTail,
  insertMessageBeforeAnchor,
  mergeHydratedMessages,
  mergeMessagePage,
  messagesInCurrentPageWindow,
  countRunningBackgroundSubagents,
  applyPersistedBackgroundHostOutcomes,
  finalizeOrphanBackgroundHosts,
  repairBackgroundHostsFromChildOutcomes,
  normalizeInterruptedAssistantStatuses,
  normalizeStaleEndedAssistantTurn,
  removeTrailingDiscardableEmptyAssistant,
  retainIncomingNewerMessages,
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

  it('insertMessageBeforeAnchor keeps the recorded tool-row anchor', () => {
    const toolOnly: ChatMessage = {
      id: 't1',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 0,
      toolCalls: [{ id: 'tc1', name: 'skill_read', status: 'success', arguments: '{}' }]
    }
    const c = conv([
      { id: 'u1', role: 'user', content: 'q', status: 'done', createdAt: 0 },
      toolOnly,
      { id: 'a1', role: 'assistant', content: 'done', status: 'done', createdAt: 0, toolCalls: [] }
    ])
    insertMessageBeforeAnchor(c, 't1', {
      id: 'sum',
      role: 'user',
      content: '[Conversation summary (auto-compression)]\nbody',
      status: 'done',
      createdAt: 0
    })
    expect(c.messages.map(m => m.id)).toEqual(['u1', 'sum', 't1', 'a1'])
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

  it('closeAbandonedEmptyAssistantShells finishes empty streaming rows except keepId', () => {
    const c = conv([
      {
        id: 'old',
        role: 'assistant',
        content: '',
        status: 'streaming',
        contentStreaming: true,
        createdAt: 0,
        toolCalls: []
      },
      {
        id: 'keep',
        role: 'assistant',
        content: '',
        status: 'streaming',
        contentStreaming: true,
        createdAt: 1,
        toolCalls: []
      }
    ])
    closeAbandonedEmptyAssistantShells(c, 'keep')
    expect(c.messages[0].status).toBe('done')
    expect(c.messages[0].contentStreaming).toBe(false)
    expect(c.messages[1].status).toBe('streaming')
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

  it('normalizeInterruptedAssistantStatuses keeps background subagent rows running', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: 'parent done',
        status: 'done',
        createdAt: 0,
        toolCalls: [
          {
            id: 'bg1',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true }),
            status: 'running',
            result: '{"jobId":"job_1","status":"running","kind":"subagent"}'
          }
        ],
        agentTrace: [
          {
            id: 'sub-bg',
            name: 'explore',
            role: 'sub',
            status: 'running',
            depth: 1,
            parentToolCallId: 'bg1',
            session: {
              contentStreaming: true,
              collapsed: true,
              userExpanded: false,
              stats: { searchCount: 0, readCount: 0 },
              toolCalls: [{ id: 't1', name: 'file_read', arguments: '{}', status: 'running' }]
            }
          }
        ]
      }
    ])
    normalizeInterruptedAssistantStatuses([c])
    expect(c.messages[0].toolCalls![0].status).toBe('running')
    expect(c.messages[0].agentTrace![0].status).toBe('running')
    expect(c.messages[0].agentTrace![0].session!.contentStreaming).toBe(true)
    expect(c.messages[0].agentTrace![0].session!.toolCalls![0].status).toBe('running')
  })

  it('countRunningBackgroundSubagents only counts in-progress host rows', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: 'done',
        status: 'done',
        createdAt: 0,
        toolCalls: [
          {
            id: 'bg1',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true }),
            status: 'success',
            result: '{"jobId":"job_1","status":"completed","kind":"subagent"}'
          },
          {
            id: 'bg2',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true }),
            status: 'running',
            result: '{"jobId":"job_2","status":"running","kind":"subagent"}'
          }
        ]
      }
    ])
    expect(countRunningBackgroundSubagents(c)).toBe(1)
  })

  it('finalizeOrphanBackgroundHosts marks leftover background hosts interrupted', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: 'done',
        status: 'done',
        createdAt: 0,
        toolCalls: [
          {
            id: 'bg1',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true }),
            status: 'running',
            result: '{"jobId":"job_1","status":"running","kind":"subagent"}'
          }
        ],
        agentTrace: [
          {
            id: 'sub-bg',
            name: 'explore',
            role: 'sub',
            status: 'running',
            depth: 1,
            parentToolCallId: 'bg1',
            session: {
              contentStreaming: true,
              collapsed: true,
              userExpanded: false,
              stats: { searchCount: 0, readCount: 0 },
              toolCalls: []
            }
          }
        ]
      }
    ])
    expect(finalizeOrphanBackgroundHosts(c)).toBe(1)
    expect(c.messages[0].toolCalls![0].status).toBe('failed')
    expect(c.messages[0].toolCalls![0].error).toBe('interrupted')
    expect(c.messages[0].agentTrace![0].status).toBe('completed')
    expect(countRunningBackgroundSubagents(c)).toBe(0)
  })

  it('finalizeOrphanBackgroundHosts promotes completed handles to success', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: 'done',
        status: 'done',
        createdAt: 0,
        toolCalls: [
          {
            id: 'bg1',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true }),
            status: 'running',
            result: '{"jobId":"job_1","status":"completed","kind":"subagent"}'
          }
        ]
      }
    ])
    expect(finalizeOrphanBackgroundHosts(c)).toBe(1)
    expect(c.messages[0].toolCalls![0].status).toBe('success')
    expect(c.messages[0].toolCalls![0].error).toBeUndefined()
    expect(countRunningBackgroundSubagents(c)).toBe(0)
  })

  it('applyPersistedBackgroundHostOutcomes copies terminal hosts from disk', () => {
    const live = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: 'done',
        status: 'done',
        createdAt: 0,
        toolCalls: [
          {
            id: 'bg1',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true }),
            status: 'running',
            result: '{"jobId":"job_1","status":"running","kind":"subagent"}'
          }
        ]
      }
    ])
    const persisted: typeof live.messages = [
      {
        ...live.messages[0]!,
        toolCalls: [
          {
            id: 'bg1',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true }),
            status: 'success',
            result: '{"jobId":"job_1","status":"completed","kind":"subagent"}'
          }
        ]
      }
    ]
    expect(applyPersistedBackgroundHostOutcomes(live, persisted)).toBe(1)
    expect(live.messages[0]!.toolCalls![0]!.status).toBe('success')
    expect(live.messages[0]!.toolCalls![0]!.result).toContain('"completed"')
  })

  it('repairBackgroundHostsFromChildOutcomes promotes hosts when traces finished', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: '清单已备',
        status: 'streaming',
        createdAt: 0,
        toolCalls: [
          {
            id: 'call_00_host',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', background: true }),
            status: 'running',
            result: '{"jobId":"job_1","status":"running","kind":"subagent"}'
          }
        ],
        agentTrace: [
          {
            id: 'call_00_host:explore',
            name: 'explore',
            role: '',
            status: 'completed',
            depth: 1
          }
        ]
      }
    ])
    expect(repairBackgroundHostsFromChildOutcomes(c)).toBe(1)
    expect(c.messages[0]!.toolCalls![0]!.status).toBe('success')
    expect(c.messages[0]!.toolCalls![0]!.result).toContain('"completed"')
    expect(c.messages[0]!.agentTrace![0]!.parentToolCallId).toBe('call_00_host')
    expect(c.messages[0]!.status).toBe('done')
  })

  it('finalizeOrphanBackgroundHosts does not cancel hosts with completed children', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: 'done',
        status: 'done',
        createdAt: 0,
        toolCalls: [
          {
            id: 'bg1',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', background: true }),
            status: 'running',
            result: '{"jobId":"job_1","status":"running","kind":"subagent"}'
          }
        ],
        agentTrace: [
          {
            id: 'bg1:explore',
            name: 'explore',
            role: '',
            status: 'completed',
            depth: 1,
            parentToolCallId: 'bg1'
          }
        ]
      }
    ])
    expect(finalizeOrphanBackgroundHosts(c)).toBe(1)
    expect(c.messages[0]!.toolCalls![0]!.status).toBe('success')
    expect(c.messages[0]!.toolCalls![0]!.error).toBeUndefined()
  })

  it('applyPersistedBackgroundHostOutcomes leaves disk-running hosts alone', () => {
    const live = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: '',
        status: 'done',
        createdAt: 0,
        toolCalls: [
          {
            id: 'bg1',
            name: 'run_subagent',
            arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true }),
            status: 'running',
            result: '{"jobId":"job_1","status":"running","kind":"subagent"}'
          }
        ]
      }
    ])
    expect(applyPersistedBackgroundHostOutcomes(live, live.messages)).toBe(0)
    expect(live.messages[0]!.toolCalls![0]!.status).toBe('running')
  })

  it('normalizeInterruptedAssistantStatuses keeps background terminal rows running', () => {
    const c = conv([
      {
        id: 'a1',
        role: 'assistant',
        content: 'parent done',
        status: 'done',
        createdAt: 0,
        toolCalls: [
          {
            id: 'bg-term',
            name: 'terminal',
            arguments: JSON.stringify({ command: 'cargo test', blockUntilMs: 0 }),
            status: 'running',
            result: '{"jobId":"job_2","status":"running","kind":"terminal"}'
          }
        ]
      }
    ])
    normalizeInterruptedAssistantStatuses([c])
    expect(c.messages[0].toolCalls![0].status).toBe('running')
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

  it('mergeHydratedMessages keeps earlier live turns before the DB tail window', () => {
    const hello: ChatMessage = {
      id: 'hello',
      role: 'user',
      content: '哈喽',
      status: 'done',
      createdAt: 1,
      position: 0
    }
    const helloReply: ChatMessage = {
      id: 'hello-a',
      role: 'assistant',
      content: 'hi',
      status: 'done',
      createdAt: 2,
      position: 1
    }
    const work: ChatMessage = {
      id: 'work',
      role: 'user',
      content: '报销',
      status: 'done',
      createdAt: 100,
      position: 40
    }
    const workStream: ChatMessage = {
      id: 'work-a',
      role: 'assistant',
      content: '处理中',
      status: 'streaming',
      createdAt: 101,
      position: 41
    }
    const merged = mergeHydratedMessages(
      [hello, helloReply, work, workStream],
      [work, { ...workStream, status: 'done' }]
    )
    expect(merged.map(m => m.id)).toEqual(['hello', 'hello-a', 'work', 'work-a'])
    expect(merged[3]?.status).toBe('streaming')
  })

  it('mergeHydratedMessages in-flight-tail drops around-window rows on a force tail', () => {
    const mid: ChatMessage = {
      id: 'mid-u',
      role: 'user',
      content: '中间',
      status: 'done',
      createdAt: 10,
      position: 19
    }
    const midReply: ChatMessage = {
      id: 'mid-a',
      role: 'assistant',
      content: 'ok',
      status: 'done',
      createdAt: 11,
      position: 20
    }
    const work: ChatMessage = {
      id: 'work',
      role: 'user',
      content: '报销',
      status: 'done',
      createdAt: 100,
      position: 40
    }
    const workStream: ChatMessage = {
      id: 'work-a',
      role: 'assistant',
      content: '处理中',
      status: 'streaming',
      createdAt: 101,
      position: 41
    }
    const merged = mergeHydratedMessages(
      [mid, midReply, work, workStream],
      [work, { ...workStream, status: 'done' }],
      'in-flight-tail'
    )
    expect(merged.map(m => m.id)).toEqual(['work', 'work-a'])
    expect(merged[1]?.status).toBe('streaming')
  })

  it('mergeHydratedMessages in-flight-tail keeps the lead when only a scoped child is live', () => {
    const mid: ChatMessage = {
      id: 'mid-u',
      role: 'user',
      content: '中间',
      status: 'done',
      createdAt: 10,
      position: 19
    }
    const midReply: ChatMessage = {
      id: 'mid-a',
      role: 'assistant',
      content: 'ok',
      status: 'done',
      createdAt: 11,
      position: 20
    }
    const work: ChatMessage = {
      id: 'work',
      role: 'user',
      content: '报销',
      status: 'done',
      createdAt: 100,
      position: 40
    }
    const lead: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '开子任务',
      status: 'streaming',
      createdAt: 101,
      position: 41,
      agentTrace: [
        { id: 'call_1:coder', name: 'coder', role: '', status: 'running', depth: 1 }
      ]
    }
    const stub: ChatMessage = {
      id: 'sub_task_1',
      role: 'user',
      content:
        'Begin. Your assigned task is in the system prompt under **Assigned task**. Execute to completion; hand off in final assistant Markdown.',
      status: 'done',
      createdAt: 102,
      anchorMessageId: 'lead',
      traceId: 'call_1:coder'
    }
    const child: ChatMessage = {
      id: 'agent_msg_live',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 103,
      contentStreaming: true,
      anchorMessageId: 'lead',
      traceId: 'call_1:coder'
    }
    const merged = mergeHydratedMessages(
      [mid, midReply, work, lead, stub, child],
      [mid, midReply],
      'in-flight-tail'
    )
    expect(merged.map(m => m.id)).toEqual([
      'mid-u',
      'mid-a',
      'work',
      'lead',
      'sub_task_1',
      'agent_msg_live'
    ])
    expect(merged.find(m => m.id === 'lead')?.agentTrace?.[0]?.status).toBe('running')
  })

  it('mergeHydratedMessages in-flight-tail keeps the generating turn on an around page', () => {
    const mid: ChatMessage = {
      id: 'mid-u',
      role: 'user',
      content: '中间',
      status: 'done',
      createdAt: 10,
      position: 19
    }
    const midReply: ChatMessage = {
      id: 'mid-a',
      role: 'assistant',
      content: 'ok',
      status: 'done',
      createdAt: 11,
      position: 20
    }
    const work: ChatMessage = {
      id: 'work',
      role: 'user',
      content: '报销',
      status: 'done',
      createdAt: 100,
      position: 40
    }
    const workStream: ChatMessage = {
      id: 'work-a',
      role: 'assistant',
      content: '处理中',
      status: 'streaming',
      createdAt: 101,
      position: 41
    }
    const merged = mergeHydratedMessages(
      [mid, midReply, work, workStream],
      [mid, midReply],
      'in-flight-tail'
    )
    expect(merged.map(m => m.id)).toEqual(['mid-u', 'mid-a', 'work', 'work-a'])
    expect(merged[3]?.status).toBe('streaming')
  })

  it('mergeMessagePage does not append an older page below newer turns', () => {
    const work: ChatMessage = {
      id: 'work',
      role: 'user',
      content: '报销',
      status: 'done',
      createdAt: 100,
      position: 40
    }
    const hello: ChatMessage = {
      id: 'hello',
      role: 'user',
      content: '哈喽',
      status: 'done',
      createdAt: 1,
      position: 0
    }
    const merged = mergeMessagePage([work], [hello], 'newer')
    expect(merged.map(m => m.id)).toEqual(['work'])
  })

  it('mergeMessagePage newer keeps mid-window rows when given the page cursor', () => {
    const mid: ChatMessage = {
      id: 'mid',
      role: 'user',
      content: '中间',
      status: 'done',
      createdAt: 10,
      position: 20
    }
    const live: ChatMessage = {
      id: 'live',
      role: 'assistant',
      content: '…',
      status: 'streaming',
      createdAt: 200,
      position: 90
    }
    const next: ChatMessage = {
      id: 'next',
      role: 'user',
      content: '下一页',
      status: 'done',
      createdAt: 30,
      position: 21
    }
    expect(
      mergeMessagePage([mid, live], [next], 'newer', { afterPosition: 20 }).map(m => m.id)
    ).toEqual(['mid', 'next', 'live'])
  })

  it('retainIncomingNewerMessages drops earlier turns even when position is higher', () => {
    const work: ChatMessage = {
      id: 'work',
      role: 'user',
      content: '报销',
      status: 'done',
      createdAt: 100,
      position: 40
    }
    const hello: ChatMessage = {
      id: 'hello',
      role: 'user',
      content: '哈喽',
      status: 'done',
      createdAt: 1,
      position: 500
    }
    const later: ChatMessage = {
      id: 'later',
      role: 'user',
      content: '继续',
      status: 'done',
      createdAt: 120,
      position: 41
    }
    expect(retainIncomingNewerMessages([work], [hello, later]).map(m => m.id)).toEqual(['later'])
  })

  it('retainIncomingNewerMessages uses the page cursor, not a disconnected live tail', () => {
    const mid: ChatMessage = {
      id: 'mid',
      role: 'user',
      content: '中间',
      status: 'done',
      createdAt: 10,
      position: 20
    }
    const live: ChatMessage = {
      id: 'live',
      role: 'assistant',
      content: '…',
      status: 'streaming',
      createdAt: 200,
      position: 90
    }
    const next: ChatMessage = {
      id: 'next',
      role: 'user',
      content: '下一页',
      status: 'done',
      createdAt: 30,
      position: 21
    }
    expect(
      retainIncomingNewerMessages([mid, live], [next], { afterPosition: 20 }).map(m => m.id)
    ).toEqual(['next'])
  })

  it('hasDisconnectedLiveTail is true when generating sits past the page cursor', () => {
    const mid: ChatMessage = {
      id: 'mid',
      role: 'user',
      content: '中间',
      status: 'done',
      createdAt: 10,
      position: 20
    }
    const live: ChatMessage = {
      id: 'live',
      role: 'assistant',
      content: '…',
      status: 'streaming',
      createdAt: 200,
      position: 90
    }
    expect(hasDisconnectedLiveTail([mid, live], 20)).toBe(true)
    expect(hasDisconnectedLiveTail([mid], 20)).toBe(false)
    expect(conversationNeedsTailReload(false, [mid, live], 20)).toBe(true)
    expect(conversationNeedsTailReload(false, [mid], 20)).toBe(false)
    expect(conversationNeedsTailReload(true, [mid], 20)).toBe(true)
  })

  it('messagesInCurrentPageWindow drops disconnected live rows from a hole', () => {
    const mid: ChatMessage = {
      id: 'mid',
      role: 'user',
      content: '中间',
      status: 'done',
      createdAt: 10,
      position: 20
    }
    const live: ChatMessage = {
      id: 'live',
      role: 'assistant',
      content: '…',
      status: 'streaming',
      createdAt: 200,
      position: 90
    }
    const liveNoPos: ChatMessage = {
      id: 'live-np',
      role: 'assistant',
      content: '…',
      status: 'streaming',
      createdAt: 201
    }
    const page = { hasMoreNewer: true, newestPosition: 20 }
    expect(messagesInCurrentPageWindow([mid, live, liveNoPos], page).map(m => m.id)).toEqual(['mid'])
    expect(
      messagesInCurrentPageWindow([mid, live], { hasMoreNewer: false, newestPosition: 20 }).map(m => m.id)
    ).toEqual(['mid', 'live'])
    expect(
      messagesInCurrentPageWindow([mid, live], { hasMoreNewer: true, newestPosition: null }).map(m => m.id)
    ).toEqual(['mid', 'live'])
  })
})
