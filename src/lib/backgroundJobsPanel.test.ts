import { describe, expect, it } from 'vitest'
import type { ChatMessage, ToolCall } from '../types/chat'
import { collectLiveBackgroundJobs } from './backgroundJobsPanel'

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

describe('collectLiveBackgroundJobs', () => {
  it('collects a running background sub-agent host once across messages', () => {
    const host = tc({
      id: 'host-1',
      name: 'run_subagent',
      status: 'running',
      arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true })
    })
    const items = collectLiveBackgroundJobs([assistantMessage([host]), assistantMessage([host])])
    expect(items).toHaveLength(1)
    expect(items[0]?.kind).toBe('host')
    expect(items[0]?.toolCall.id).toBe('host-1')
  })

  it('ignores finished hosts and non-background tools', () => {
    const done = tc({
      id: 'done-1',
      name: 'run_subagent',
      status: 'success',
      arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true })
    })
    const plain = tc({ id: 'plain-1', name: 'file_read', status: 'running' })
    const items = collectLiveBackgroundJobs([assistantMessage([done, plain])])
    expect(items).toEqual([])
  })

  it('collects a running job.await tool as await kind', () => {
    const awaitCall = tc({
      id: 'await-1',
      name: 'job',
      status: 'running',
      arguments: JSON.stringify({ action: 'await', jobId: 'job_abc' })
    })
    const items = collectLiveBackgroundJobs([assistantMessage([awaitCall])])
    expect(items).toHaveLength(1)
    expect(items[0]?.kind).toBe('await')
  })

  it('returns empty for undefined messages', () => {
    expect(collectLiveBackgroundJobs(undefined)).toEqual([])
  })
})
