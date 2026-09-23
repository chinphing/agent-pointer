import { describe, expect, it } from 'vitest'
import type { ChatMessage, Conversation, ToolCall } from '../types/chat'
import {
  bindUnboundTracesToHosts,
  buildSubAgentBodyModelsFromScoped,
  buildSubAgentBodyModelsSplitAtCut,
  computeSubAgentStatsFromMessages,
  ensureHostLinkedSubTraces,
  ensureScopedChildMessage,
  isSubAgentHostStubContent,
  mergeSubAgentToolCalls,
  rehydrateAgentTracesFromScopedMessages,
  scopedAssistantMessagesForTrace,
  scopedMessagesForTrace,
  subAgentFrameOwnsCompression
} from './subAgentMessages'
import { formatSubAgentSummaryLine } from './subAgentStats'

describe('mergeSubAgentToolCalls', () => {
  function tc(over: Partial<ToolCall> & Pick<ToolCall, 'id'>): ToolCall {
    return {
      name: 'ask_user',
      status: 'running',
      arguments: '',
      ...over
    }
  }

  it('keeps the session copy when scoped arguments cannot render options', () => {
    const scoped = tc({
      id: 'ask',
      arguments: ''
    })
    const session = tc({
      id: 'ask',
      arguments: JSON.stringify({
        question: '选哪个？',
        options: [{ label: 'A' }, { label: 'B' }]
      }),
      displaySummary: '选哪个？\n1. A\n2. B'
    })
    const merged = mergeSubAgentToolCalls([[scoped], [session]])
    expect(merged).toHaveLength(1)
    expect(merged[0]?.arguments).toContain('选哪个')
  })
})

describe('rehydrateAgentTracesFromScopedMessages', () => {
  it('stamps live scoped child rows with the agent instance id', () => {
    const conv: Conversation = {
      id: 'c1',
      title: 't',
      createdAt: 1,
      updatedAt: 1,
      messages: [],
      skillIds: []
    }

    const child = ensureScopedChildMessage(conv, 'lead', 'child', {
      traceId: 'task:explore',
      taskId: 'task',
      spawnDepth: 1,
      agentInstanceId: 'instance-current'
    })

    expect(child.agentInstanceId).toBe('instance-current')
  })

  it('isolates reused anchor and trace by agent instance while preserving legacy lookup', () => {
    const messages = [
      {
        id: 'old',
        role: 'assistant' as const,
        content: 'old result',
        status: 'done' as const,
        createdAt: 1,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        agentInstanceId: 'instance-old'
      },
      {
        id: 'current',
        role: 'assistant' as const,
        content: 'current result',
        status: 'done' as const,
        createdAt: 2,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        agentInstanceId: 'instance-current'
      }
    ]

    expect(
      scopedMessagesForTrace(messages, 'lead', 'task:explore', 'instance-current')
        .map(message => message.id)
    ).toEqual(['current'])
    expect(
      scopedAssistantMessagesForTrace(
        messages,
        'lead',
        'task:explore',
        'instance-current'
      ).map(message => message.id)
    ).toEqual(['current'])
    expect(scopedMessagesForTrace(messages, 'lead', 'task:explore')).toHaveLength(2)
  })

  it('matches process rows by spawn id when AgentTrace.id is the instance', () => {
    const instance = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'
    const messages = [
      {
        id: 'row',
        role: 'assistant' as const,
        content: 'working',
        status: 'streaming' as const,
        createdAt: 1,
        anchorMessageId: 'lead',
        traceId: 'task:coder',
        agentInstanceId: instance
      }
    ]
    expect(
      scopedMessagesForTrace(messages, 'lead', instance, instance).map(m => m.id)
    ).toEqual(['row'])
    expect(
      scopedMessagesForTrace(messages, 'lead', instance).map(m => m.id)
    ).toEqual(['row'])
  })

  it('rebuilds agentTrace index from scoped child rows', () => {
    const conv: Conversation = {
      id: 'c1',
      title: 't',
      createdAt: 1,
      updatedAt: 1,
      messages: [
        {
          id: 'lead',
          role: 'assistant',
          content: 'done',
          status: 'done',
          createdAt: 1
        },
        {
          id: 'sub1',
          role: 'assistant',
          content: 'explored repo',
          status: 'done',
          createdAt: 2,
          anchorMessageId: 'lead',
          traceId: 'task_a:explore',
          taskId: 'task_a',
          spawnDepth: 1,
          agentName: 'Explore'
        }
      ],
      skillIds: []
    }

    rehydrateAgentTracesFromScopedMessages(conv)
    const lead = conv.messages[0]
    expect(lead.agentTrace).toHaveLength(1)
    expect(lead.agentTrace![0].id).toBe('task_a:explore')
    expect(lead.agentTrace![0].status).toBe('completed')
    expect(lead.agentTrace![0].name).toBe('Explore')
  })

  it('rehydrates reused trace with the latest agent instance', () => {
    const conv: Conversation = {
      id: 'c1',
      title: 't',
      createdAt: 1,
      updatedAt: 1,
      messages: [
        {
          id: 'lead',
          role: 'assistant',
          content: '',
          status: 'done',
          createdAt: 1
        },
        {
          id: 'old',
          role: 'assistant',
          content: 'old',
          status: 'done',
          createdAt: 2,
          anchorMessageId: 'lead',
          traceId: 'task_a:explore',
          taskId: 'task_a',
          spawnDepth: 1,
          agentInstanceId: 'instance-old'
        },
        {
          id: 'current',
          role: 'assistant',
          content: 'current',
          status: 'done',
          createdAt: 3,
          anchorMessageId: 'lead',
          traceId: 'task_a:explore',
          taskId: 'task_a',
          spawnDepth: 1,
          agentInstanceId: 'instance-current'
        }
      ],
      skillIds: []
    }

    rehydrateAgentTracesFromScopedMessages(conv)

    expect(conv.messages[0].agentTrace![0].agentInstanceId).toBe('instance-current')
    expect(conv.messages[0].agentTrace![0].detail).toBe('current')
  })

  it('does not duplicate existing trace rows', () => {
    const conv: Conversation = {
      id: 'c1',
      title: 't',
      createdAt: 1,
      updatedAt: 1,
      messages: [
        {
          id: 'lead',
          role: 'assistant',
          content: '',
          status: 'done',
          createdAt: 1,
          agentTrace: [
            {
              id: 'task_a:explore',
              name: 'Explore',
              role: '',
              status: 'completed',
              depth: 1
            }
          ]
        },
        {
          id: 'sub1',
          role: 'assistant',
          content: 'x',
          status: 'done',
          createdAt: 2,
          anchorMessageId: 'lead',
          traceId: 'task_a:explore',
          taskId: 'task_a',
          spawnDepth: 1
        }
      ],
      skillIds: []
    }

    rehydrateAgentTracesFromScopedMessages(conv)
    expect(conv.messages[0].agentTrace).toHaveLength(1)
  })

  it('binds traces to background hosts and uses the latest child status', () => {
    const conv: Conversation = {
      id: 'c1',
      title: 't',
      createdAt: 1,
      updatedAt: 1,
      messages: [
        {
          id: 'lead',
          role: 'assistant',
          content: '开 4 个后台',
          status: 'streaming',
          createdAt: 1,
          toolCalls: [
            {
              id: 'call_00',
              name: 'run_subagent',
              arguments: JSON.stringify({ agentId: 'explore', background: true }),
              status: 'running'
            },
            {
              id: 'call_01',
              name: 'run_subagent',
              arguments: JSON.stringify({ agentId: 'explore', background: true }),
              status: 'running'
            }
          ]
        },
        {
          id: 'mid',
          role: 'assistant',
          content: '',
          status: 'streaming',
          createdAt: 2,
          anchorMessageId: 'lead',
          traceId: 'call_00:explore',
          taskId: 'call_00'
        },
        {
          id: 'last',
          role: 'assistant',
          content: '交接完成',
          status: 'done',
          createdAt: 3,
          anchorMessageId: 'lead',
          traceId: 'call_00:explore',
          taskId: 'call_00'
        },
        {
          id: 'b2',
          role: 'assistant',
          content: '第二路完成',
          status: 'done',
          createdAt: 4,
          anchorMessageId: 'lead',
          traceId: 'call_01:explore',
          taskId: 'call_01'
        }
      ],
      skillIds: []
    }
    rehydrateAgentTracesFromScopedMessages(conv)
    const traces = conv.messages[0]!.agentTrace ?? []
    expect(traces).toHaveLength(2)
    expect(traces.find(t => t.id === 'call_00:explore')?.status).toBe('completed')
    expect(traces.find(t => t.id === 'call_00:explore')?.parentToolCallId).toBe('call_00')
    expect(traces.find(t => t.id === 'call_01:explore')?.parentToolCallId).toBe('call_01')
  })

  it('does not stamp a live running trace failed from persisted children', () => {
    const conv: Conversation = {
      id: 'c1',
      title: 't',
      createdAt: 1,
      updatedAt: 1,
      messages: [
        {
          id: 'lead',
          role: 'assistant',
          content: '开子任务',
          status: 'streaming',
          createdAt: 1,
          agentTrace: [
            {
              id: 'call_1:coder',
              name: 'coder',
              role: '',
              status: 'running',
              depth: 1,
              parentToolCallId: 'call_1'
            }
          ]
        },
        {
          id: 'old-error',
          role: 'assistant',
          content: 'x',
          status: 'error',
          createdAt: 2,
          anchorMessageId: 'lead',
          traceId: 'call_1:coder'
        }
      ],
      skillIds: []
    }
    rehydrateAgentTracesFromScopedMessages(conv)
    expect(conv.messages[0]!.agentTrace![0]!.status).toBe('running')
  })

  it('filters host stub and tool rows from sub-agent display', () => {
    const messages = [
      {
        id: 'stub',
        role: 'user' as const,
        content:
          'Begin. Your assigned task is in the system prompt under **Assigned task**.',
        status: 'done' as const,
        createdAt: 1,
        anchorMessageId: 'lead',
        traceId: 't:computer'
      },
      {
        id: 'tool1',
        role: 'tool' as const,
        content: '{"ok":true}',
        status: 'done' as const,
        createdAt: 2,
        anchorMessageId: 'lead',
        traceId: 't:computer',
        toolCallId: 'call_1'
      },
      {
        id: 'a1',
        role: 'assistant' as const,
        content: 'Goal: open app',
        status: 'done' as const,
        createdAt: 3,
        anchorMessageId: 'lead',
        traceId: 't:computer',
        toolCalls: [{ id: 'call_1', name: 'launch_app', arguments: '{}', status: 'success' as const }]
      },
      {
        id: 'a2',
        role: 'assistant' as const,
        content: '微信已成功打开。',
        status: 'done' as const,
        createdAt: 4,
        anchorMessageId: 'lead',
        traceId: 't:computer'
      }
    ]
    expect(scopedAssistantMessagesForTrace(messages, 'lead', 't:computer')).toHaveLength(2)
    const bodies = buildSubAgentBodyModelsFromScoped(messages, 'lead', 't:computer', 'completed')
    expect(bodies).toHaveLength(1)
    expect(bodies[0].content).toBe('微信已成功打开。')
    expect(bodies[0].toolCalls).toHaveLength(1)
    expect(isSubAgentHostStubContent(messages[0].content)).toBe(true)
  })

  it('splits process bodies at the keep-window cut', () => {
    const messages = [
      {
        id: 'dropped',
        role: 'assistant' as const,
        content: '',
        status: 'done' as const,
        createdAt: 1,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        toolCalls: [{ id: 't1', name: 'skill_read', arguments: '{}', status: 'success' as const }]
      },
      {
        id: 'keep',
        role: 'assistant' as const,
        content: '',
        status: 'done' as const,
        createdAt: 2,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        toolCalls: [{ id: 't2', name: 'skill_edit', arguments: '{}', status: 'success' as const }]
      }
    ]
    const split = buildSubAgentBodyModelsSplitAtCut(
      messages,
      'lead',
      'task:explore',
      'keep',
      'running'
    )
    expect(split.cutFound).toBe(true)
    expect(split.before[0]?.toolCalls?.map(tc => tc.id)).toEqual(['t1'])
    expect(split.after[0]?.toolCalls?.map(tc => tc.id)).toEqual(['t2'])
  })

  it('keeps all process rows before the marker when the cut id is missing', () => {
    const messages = [
      {
        id: 'a1',
        role: 'assistant' as const,
        content: '',
        status: 'done' as const,
        createdAt: 1,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        toolCalls: [{ id: 't1', name: 'skill_read', arguments: '{}', status: 'success' as const }]
      }
    ]
    const split = buildSubAgentBodyModelsSplitAtCut(
      messages,
      'lead',
      'task:explore',
      'missing-keep',
      'running'
    )
    expect(split.cutFound).toBe(false)
    expect(split.after).toEqual([])
    expect(split.before[0]?.toolCalls?.map(tc => tc.id)).toEqual(['t1'])
  })

  it('owns compression only for the trace that contains the cut', () => {
    const messages = [
      {
        id: 'keep',
        role: 'assistant' as const,
        content: '',
        status: 'done' as const,
        createdAt: 1,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        agentInstanceId: 'inst-a'
      },
      {
        id: 'other',
        role: 'assistant' as const,
        content: '',
        status: 'done' as const,
        createdAt: 2,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        agentInstanceId: 'inst-b'
      }
    ]
    const state = {
      scope: 'sub_agent',
      insertBeforeMessageId: 'keep',
      messageId: 'lead',
      subAgentId: 'explore'
    }
    expect(
      subAgentFrameOwnsCompression(state, {
        messages,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        agentInstanceId: 'inst-a'
      })
    ).toBe(true)
    expect(
      subAgentFrameOwnsCompression(state, {
        messages,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        agentInstanceId: 'inst-b'
      })
    ).toBe(false)
    expect(
      subAgentFrameOwnsCompression(state, {
        messages,
        anchorMessageId: 'lead',
        traceId: 'inst-a',
        agentInstanceId: 'inst-a'
      })
    ).toBe(true)

    const ownOnly = messages.filter(m => m.agentInstanceId === 'inst-b')
    const keep = messages.find(m => m.id === 'keep')
    expect(
      subAgentFrameOwnsCompression(state, {
        messages: ownOnly,
        cutScopedRow: keep,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        agentInstanceId: 'inst-b'
      })
    ).toBe(false)
    expect(
      subAgentFrameOwnsCompression(state, {
        messages: [],
        cutScopedRow: keep,
        anchorMessageId: 'lead',
        traceId: 'task:explore',
        agentInstanceId: 'inst-a'
      })
    ).toBe(true)
  })

  it('counts tool stats from persisted pending status when tool result row exists', () => {
    const messages = [
      {
        id: 'a1',
        role: 'assistant' as const,
        content: 'opening',
        status: 'done' as const,
        createdAt: 1,
        anchorMessageId: 'lead',
        traceId: 't:computer',
        toolCalls: [{ id: 'call_1', name: 'launch_app', arguments: '{}', status: 'pending' as const }]
      },
      {
        id: 'tool1',
        role: 'tool' as const,
        content: '{"ok":true}',
        status: 'done' as const,
        createdAt: 2,
        anchorMessageId: 'lead',
        traceId: 't:computer',
        toolCallId: 'call_1'
      }
    ]
    const stats = computeSubAgentStatsFromMessages(messages)
    const line = formatSubAgentSummaryLine('电脑操控', 'completed', stats, 'computer')
    expect(line).toContain('其他 1 次')
    expect(line).not.toContain('工具 0 次')
  })
})

describe('ensureHostLinkedSubTraces', () => {
  it('pairs unbound traces to foreground run_subagent hosts', () => {
    const lead: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      toolCalls: [
        {
          id: 'call-fg',
          name: 'run_subagent',
          arguments: '{"agentId":"coder","goal":"补跑"}',
          status: 'success'
        }
      ],
      agentTrace: [
        {
          id: 'inst-1',
          name: '',
          role: '',
          status: 'completed',
          depth: 1,
          agentInstanceId: 'inst-1'
        }
      ]
    }
    bindUnboundTracesToHosts(lead)
    expect(lead.agentTrace![0]!.parentToolCallId).toBe('call-fg')
  })

  it('synthesizes a nestable trace from host result when agentTrace is missing', () => {
    const lead: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      toolCalls: [
        {
          id: 'call-fg',
          name: 'run_subagent',
          arguments: '{"agentId":"coder","goal":"补跑835113"}',
          status: 'success',
          result: JSON.stringify({ agentInstanceId: 'inst-hydrated', content: 'ok' })
        }
      ]
    }
    ensureHostLinkedSubTraces(lead)
    expect(lead.agentTrace).toHaveLength(1)
    expect(lead.agentTrace![0]!.id).toBe('inst-hydrated')
    expect(lead.agentTrace![0]!.parentToolCallId).toBe('call-fg')
    expect(lead.agentTrace![0]!.agentInstanceId).toBe('inst-hydrated')
  })
})
