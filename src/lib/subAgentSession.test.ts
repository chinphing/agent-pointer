import { describe, expect, it } from 'vitest'
import type { AgentTrace, ChatMessage } from '../types/chat'
import {
  ensureSubTrace,
  isSubAgentTraceTerminal,
  isSubTraceUiCollapsed,
  orphanSubTraces,
  subTracesForParentToolCall,
  toggleSubTraceExpanded
} from './subAgentSession'

describe('sub-agent frame collapse', () => {
  function runningTrace(overrides: Partial<AgentTrace> = {}): AgentTrace {
    return {
      id: 'task:explore',
      name: 'Explore',
      role: 'worker',
      status: 'running',
      depth: 1,
      collapsed: true,
      userExpanded: false,
      ...overrides
    }
  }

  it('defaults to collapsed while running', () => {
    expect(isSubTraceUiCollapsed(runningTrace())).toBe(true)
  })

  it('stays collapsed after completed unless user expands', () => {
    expect(
      isSubTraceUiCollapsed(runningTrace({ status: 'completed', collapsed: true }))
    ).toBe(true)
  })

  it('opens only after user expands', () => {
    const trace = runningTrace()
    toggleSubTraceExpanded(trace)
    expect(trace.userExpanded).toBe(true)
    expect(isSubTraceUiCollapsed(trace)).toBe(false)
  })

  it('keeps manual expand across stream agent_step patches', () => {
    const message: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 1
    }
    const trace = ensureSubTrace(message, 'task:explore', {
      status: 'running',
      collapsed: true,
      userExpanded: false
    })
    toggleSubTraceExpanded(trace)
    ensureSubTrace(message, 'task:explore', {
      status: 'running',
      collapsed: true,
      userExpanded: false
    })
    expect(trace.userExpanded).toBe(true)
    expect(isSubTraceUiCollapsed(trace)).toBe(false)
  })

  it('keeps parentToolCallId when a later patch omits it', () => {
    const message: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 1
    }
    ensureSubTrace(message, 'task:explore', {
      status: 'running',
      parentToolCallId: 'call-a'
    })
    const trace = ensureSubTrace(message, 'task:explore', {
      depth: 1,
      agentInstanceId: 'inst-1'
    })
    expect(trace.parentToolCallId).toBe('call-a')
  })
})

describe('sub-agent parent tool nesting', () => {
  it('maps traces to the parent run_subagent tool call', () => {
    const traces: AgentTrace[] = [
      {
        id: 't1',
        name: 'Explore',
        role: 'worker',
        status: 'completed',
        depth: 1,
        parentToolCallId: 'call-a'
      },
      {
        id: 't2',
        name: 'Explore',
        role: 'worker',
        status: 'completed',
        depth: 1,
        parentToolCallId: 'call-b'
      }
    ]
    expect(subTracesForParentToolCall(traces, 'call-a').map(t => t.id)).toEqual(['t1'])
    expect(subTracesForParentToolCall(traces, 'call-b').map(t => t.id)).toEqual(['t2'])
  })

  it('keeps legacy traces without parentToolCallId as orphans', () => {
    const traces: AgentTrace[] = [
      {
        id: 'legacy',
        name: 'Explore',
        role: 'worker',
        status: 'completed',
        depth: 1
      },
      {
        id: 'linked',
        name: 'Explore',
        role: 'worker',
        status: 'completed',
        depth: 1,
        parentToolCallId: 'call-a'
      },
      {
        id: 'dangling',
        name: 'Explore',
        role: 'worker',
        status: 'completed',
        depth: 1,
        parentToolCallId: 'missing-call'
      }
    ]
    expect(orphanSubTraces(traces, ['call-a']).map(t => t.id)).toEqual([
      'legacy',
      'dangling'
    ])
  })
})

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

describe('isSubAgentTraceTerminal', () => {
  it('treats completed, failed, and cancelled as terminal', () => {
    expect(isSubAgentTraceTerminal('completed')).toBe(true)
    expect(isSubAgentTraceTerminal('failed')).toBe(true)
    expect(isSubAgentTraceTerminal('cancelled')).toBe(true)
    expect(isSubAgentTraceTerminal('canceled')).toBe(true)
    expect(isSubAgentTraceTerminal('running')).toBe(false)
  })
})
