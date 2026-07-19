import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import { ensureSubTrace } from './subAgentSession'

describe('ensureSubTrace', () => {
  it('retains the streamed child agent instance id', () => {
    const message: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 1
    }

    const trace = ensureSubTrace(message, 'task:explore', {
      id: 'task:explore',
      name: 'Explore',
      role: 'worker',
      status: 'running',
      agentInstanceId: 'instance-current'
    })

    expect(trace.agentInstanceId).toBe('instance-current')
  })

  it('keeps same-task self-fork instances as separate traces', () => {
    const message: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 1
    }

    const first = ensureSubTrace(message, 'reused-task:instance-one:current-agent', {
      agentInstanceId: 'instance-one'
    })
    const second = ensureSubTrace(message, 'reused-task:instance-two:current-agent', {
      agentInstanceId: 'instance-two'
    })

    expect(first).not.toBe(second)
    expect(message.agentTrace?.map(trace => trace.id)).toEqual([
      'reused-task:instance-one:current-agent',
      'reused-task:instance-two:current-agent'
    ])
  })
})
