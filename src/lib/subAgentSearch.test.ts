import { describe, expect, it, vi } from 'vitest'
import type { AgentTrace, ChatMessage } from '../types/chat'
import {
  hasSubAgentSearchTargets,
  scopedRowsContainSearchTarget,
  traceSubtreeContainsSearchTarget
} from './subAgentSearch'

function trace(over: Partial<AgentTrace> & Pick<AgentTrace, 'id'>): AgentTrace {
  return { name: 'coder', role: 'worker', status: 'completed', ...over }
}

function row(over: Partial<ChatMessage> & Pick<ChatMessage, 'id'>): ChatMessage {
  return {
    role: 'assistant',
    content: '',
    status: 'done',
    createdAt: 1,
    anchorMessageId: 'lead',
    ...over
  }
}

describe('hasSubAgentSearchTargets', () => {
  it('is false for an empty or blank target set', () => {
    expect(hasSubAgentSearchTargets({})).toBe(false)
    expect(hasSubAgentSearchTargets({ toolCallIds: [], contentMessageIds: ['  '] })).toBe(false)
    expect(hasSubAgentSearchTargets({ contentMessageIds: [' r1 '] })).toBe(true)
  })
})

describe('scopedRowsContainSearchTarget', () => {
  it('matches a searched content row and a searched tool call', () => {
    const rows = [
      row({ id: 'r1', content: '第一轮', toolCalls: [{ id: 't1', name: 'file_read', arguments: '{}', status: 'success' }] })
    ]
    expect(scopedRowsContainSearchTarget(rows, { contentMessageIds: ['r1'] })).toBe(true)
    expect(scopedRowsContainSearchTarget(rows, { toolCallIds: ['t1'] })).toBe(true)
    expect(scopedRowsContainSearchTarget(rows, { toolCallIds: ['nope'] })).toBe(false)
  })

  it('does no row work when the search is closed', () => {
    const rows = [row({ id: 'r1' })]
    expect(scopedRowsContainSearchTarget(rows, {})).toBe(false)
  })
})

describe('traceSubtreeContainsSearchTarget', () => {
  const parentTrace = trace({ id: 'inst-parent', agentInstanceId: 'inst-parent' })
  const childTrace = trace({
    id: 'inst-child',
    agentInstanceId: 'inst-child',
    parentTraceId: 'inst-parent'
  })

  function parentRows(): ChatMessage[] {
    return [
      row({
        id: 'p1',
        content: '父层第一轮',
        agentInstanceId: 'inst-parent',
        traceId: 'inst-parent',
        agentTrace: [childTrace]
      })
    ]
  }

  function childRows(): ChatMessage[] {
    return [
      row({
        id: 'c1',
        content: '孙层 handoff',
        agentInstanceId: 'inst-child',
        traceId: 'inst-child',
        anchorMessageId: 'p1'
      })
    ]
  }

  it('finds a content hit in a nested spawn so ancestors can expand', () => {
    const rowsForTrace = vi.fn((t: AgentTrace) => (t.id === 'inst-child' ? childRows() : []))
    expect(
      traceSubtreeContainsSearchTarget({
        trace: parentTrace,
        ownRows: parentRows(),
        targets: { contentMessageIds: ['c1'] },
        rowsForTrace
      })
    ).toBe(true)
    expect(rowsForTrace).toHaveBeenCalledTimes(1)
  })

  it('finds a tool hit on its own rows', () => {
    const rows = [
      row({
        id: 'p1',
        agentInstanceId: 'inst-parent',
        traceId: 'inst-parent',
        toolCalls: [{ id: 'tool-hit', name: 'terminal', arguments: '{}', status: 'success' }]
      })
    ]
    expect(
      traceSubtreeContainsSearchTarget({
        trace: parentTrace,
        ownRows: rows,
        targets: { toolCallIds: ['tool-hit'] },
        rowsForTrace: () => []
      })
    ).toBe(true)
  })

  it('stays false and skips descendant lookups when nothing is searched', () => {
    const rowsForTrace = vi.fn(() => childRows())
    expect(
      traceSubtreeContainsSearchTarget({
        trace: parentTrace,
        ownRows: parentRows(),
        targets: {},
        rowsForTrace
      })
    ).toBe(false)
    expect(rowsForTrace).not.toHaveBeenCalled()
  })

  it('stops on a cyclic parent linkage instead of recursing forever', () => {
    const a = trace({ id: 'a', parentTraceId: 'b' })
    const b = trace({ id: 'b', parentTraceId: 'a' })
    const rowsForTrace = (t: AgentTrace) =>
      t.id === 'a'
        ? [row({ id: 'a-row', agentInstanceId: 'a', traceId: 'a', agentTrace: [b] })]
        : [row({ id: 'b-row', agentInstanceId: 'b', traceId: 'b', agentTrace: [a] })]
    expect(
      traceSubtreeContainsSearchTarget({
        trace: a,
        ownRows: [row({ id: 'a-row', agentInstanceId: 'a', traceId: 'a', agentTrace: [b] })],
        targets: { contentMessageIds: ['missing'] },
        rowsForTrace
      })
    ).toBe(false)
  })
})
