// @vitest-environment happy-dom

import { beforeEach, describe, expect, it } from 'vitest'
import { handleAgentStep } from './agentHandlers'
import { createMockStreamHandlerContext, sampleAssistantMessage, sampleConversation } from './testUtils'
import { resetConversationScopedStoreForTests } from '../../../lib/conversationScoped'
import type { AgentTrace } from '../../../types/chat'

/**
 * A background `run_subagent` spawn now publishes the same `agent_step` the foreground
 * path does — anchored on the lead row, nested under the host tool call. These are the
 * two properties the frame mount depends on: the trace has to land on the **host** row
 * and keep `parentToolCallId` / `anchorMessageId` intact, or the frame has nowhere to go.
 */
function backgroundTrace(over: Partial<AgentTrace> = {}): AgentTrace {
  return {
    id: 'instance-bg',
    name: '通用助手',
    role: 'worker',
    status: 'running',
    depth: 1,
    agentInstanceId: 'instance-bg',
    parentToolCallId: 'call-bg',
    anchorMessageId: 'lead',
    userExpanded: false,
    ...over
  }
}

function leadRowWithHostToolCall() {
  const conv = sampleConversation()
  const lead = sampleAssistantMessage('lead')
  lead.toolCalls = [
    { id: 'call-bg', name: 'run_subagent', arguments: '{}', status: 'running' }
  ]
  conv.messages.push(lead)
  return { conv, lead, ctx: createMockStreamHandlerContext([conv]) }
}

beforeEach(() => {
  resetConversationScopedStoreForTests()
})

describe('handleAgentStep for a background spawn', () => {
  it('lands the running frame on the host row that owns the run_subagent tool call', () => {
    const { lead, ctx } = leadRowWithHostToolCall()

    handleAgentStep(ctx, {
      kind: 'agent_step',
      messageId: 'lead',
      agent: backgroundTrace()
    })

    const traces = lead.agentTrace ?? []
    expect(traces).toHaveLength(1)
    expect(traces[0].id).toBe('instance-bg')
    expect(traces[0].status).toBe('running')
    // The host row is found through these two ids — dropping either orphans the frame.
    expect(traces[0].parentToolCallId).toBe('call-bg')
    expect(traces[0].anchorMessageId).toBe('lead')
  })

  it('updates the same entry on the terminal step instead of adding a second frame', () => {
    const { lead, ctx } = leadRowWithHostToolCall()

    handleAgentStep(ctx, { kind: 'agent_step', messageId: 'lead', agent: backgroundTrace() })
    handleAgentStep(ctx, {
      kind: 'agent_step',
      messageId: 'lead',
      agent: backgroundTrace({ status: 'completed', detail: '调度组7~12单' })
    })

    const traces = lead.agentTrace ?? []
    expect(traces).toHaveLength(1)
    expect(traces[0].status).toBe('completed')
    expect(traces[0].parentToolCallId).toBe('call-bg')
  })

  it('keeps the frame when the lead row is already finished', () => {
    const { lead, ctx } = leadRowWithHostToolCall()
    lead.status = 'done'

    handleAgentStep(ctx, { kind: 'agent_step', messageId: 'lead', agent: backgroundTrace() })

    // A finished turn must not be revived, but the frame still has to show up.
    expect(lead.status).toBe('done')
    expect(lead.agentTrace ?? []).toHaveLength(1)
  })
})
