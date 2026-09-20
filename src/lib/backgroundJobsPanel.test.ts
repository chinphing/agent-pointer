import { describe, expect, it } from 'vitest'
import type { ChatMessage, ToolCall } from '../types/chat'
import { collectBackgroundJobsPanelItems } from './backgroundJobsPanel'

function tc(
  partial: Partial<ToolCall> & Pick<ToolCall, 'id' | 'name' | 'status'>
): ToolCall {
  return { arguments: '', ...partial }
}

function assistantMessage(toolCalls: ToolCall[]): ChatMessage {
  return {
    id: `m-${toolCalls[0]?.id ?? Math.random()}`,
    role: 'assistant',
    content: '',
    status: 'done',
    createdAt: 0,
    toolCalls
  } as ChatMessage
}

describe('collectBackgroundJobsPanelItems', () => {
  it('lists occupancy jobs including nested background terminals', () => {
    const items = collectBackgroundJobsPanelItems(
      [
        {
          jobId: 'job_sub',
          status: 'running',
          kind: 'subagent',
          title: '铺线 1',
          agentId: 'self'
        },
        {
          jobId: 'job_term',
          status: 'running',
          kind: 'terminal',
          title: 'python scrape.py'
        }
      ],
      undefined
    )
    expect(items).toHaveLength(2)
    expect(items[0]).toMatchObject({ kind: 'host', jobId: 'job_sub', title: '铺线 1' })
    expect(items[1]).toMatchObject({
      kind: 'host',
      jobId: 'job_term',
      title: 'python scrape.py',
      jobKind: 'terminal'
    })
  })

  it('does not invent host rows from parent messages', () => {
    const host = tc({
      id: 'host-1',
      name: 'run_subagent',
      status: 'running',
      arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true })
    })
    expect(collectBackgroundJobsPanelItems(undefined, [assistantMessage([host])])).toEqual([])
    expect(collectBackgroundJobsPanelItems([], [assistantMessage([host])])).toEqual([])
  })

  it('keeps await rows when occupancy already lists hosts', () => {
    const awaitCall = tc({
      id: 'await-1',
      name: 'job',
      status: 'running',
      arguments: JSON.stringify({ action: 'await', jobId: 'job_abc' })
    })
    const items = collectBackgroundJobsPanelItems(
      [{ jobId: 'job_term', status: 'running', kind: 'terminal', title: 'sleep 9' }],
      [assistantMessage([awaitCall])]
    )
    expect(items.map(i => i.kind)).toEqual(['host', 'await'])
  })
})
