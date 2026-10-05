import { describe, expect, it } from 'vitest'
import type { ChatMessage, Conversation, ToolCall } from '../types/chat'
import {
  bindUnboundTracesToHosts,
  buildSubAgentBodyModelsFromScoped,
  buildSubAgentBodyModelsSplitAtCut,
  buildSubAgentRoundsFromScoped,
  computeSubAgentStatsFromMessages,
  ensureHostLinkedSubTraces,
  ensureScopedChildMessage,
  isSubAgentHostStubContent,
  mergeSubAgentToolCalls,
  rehydrateAgentTracesFromScopedMessages,
  relocateNestedTracesToHostRows,
  scopedAssistantMessagesForTrace,
  scopedMessagesForTrace,
  subAgentFrameOwnsCompression
} from './subAgentMessages'
import { formatSubAgentSummaryLine } from './subAgentStats'
import { buildSubAgentTraceTree } from './subAgentTraceTree'
import { createConversationScopedStore } from './conversationScoped/store'

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

  it('folds interaction tools like ask_user into「其他」instead of hiding them', () => {
    // Every call the buckets do not name lands in `otherCount`, so an ask_user-only round
    // reports「其他 1 次」rather than「工具 0 次」(which used to collapse the row to「过程」).
    const messages = [
      {
        id: 'a1',
        role: 'assistant' as const,
        content: '',
        status: 'done' as const,
        createdAt: 1,
        anchorMessageId: 'lead',
        traceId: 't:general',
        toolCalls: [
          {
            id: 'call_ask',
            name: 'ask_user',
            arguments: '{"question":"选哪个？","options":[{"label":"A"},{"label":"B"}]}',
            status: 'success' as const
          }
        ]
      }
    ]
    const stats = computeSubAgentStatsFromMessages(messages)
    const line = formatSubAgentSummaryLine('测试子代理', 'completed', stats, 'general')
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

describe('relocateNestedTracesToHostRows', () => {
  function conv(messages: ChatMessage[]): Conversation {
    return { id: 'conv', title: 't', createdAt: 1, updatedAt: 1, skillIds: [], messages }
  }

  function scopedRound(over: Partial<ChatMessage> & Pick<ChatMessage, 'id'>): ChatMessage {
    return {
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 2,
      anchorMessageId: 'lead',
      agentInstanceId: 'inst-worker',
      traceId: 'sub_task_1:coder',
      spawnDepth: 1,
      ...over
    }
  }

  it('把 lead 上、宿主行在 scoped 行的 depth-2 档案搬到父 worker 行', () => {
    const lead: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      toolCalls: [
        { id: 'call-lead', name: 'run_subagent', arguments: '{}', status: 'success' }
      ],
      agentTrace: [
        { id: 'inst-worker', name: '', role: '', status: 'completed', depth: 1, parentToolCallId: 'call-lead' },
        // 记账 bug 漏到 lead 的 depth-2 档案：宿主行 call-nested 其实在 worker 的 scoped 行里。
        { id: 'inst-nested', name: '', role: '', status: 'completed', depth: 2, parentToolCallId: 'call-nested' }
      ]
    }
    const workerRow = scopedRound({
      id: 'row-worker',
      toolCalls: [{ id: 'call-nested', name: 'run_subagent', arguments: '{}', status: 'success' }]
    })

    const moved = relocateNestedTracesToHostRows(conv([lead]), [workerRow])

    expect(moved).toBe(1)
    expect(lead.agentTrace!.map(t => t.id)).toEqual(['inst-worker'])
    expect(workerRow.agentTrace!.map(t => t.id)).toEqual(['inst-nested'])
  })

  it('宿主行就在 lead 上时不搬（避免重复渲染）', () => {
    const lead: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      toolCalls: [
        { id: 'call-lead', name: 'run_subagent', arguments: '{}', status: 'success' }
      ],
      agentTrace: [
        { id: 'inst-worker', name: '', role: '', status: 'completed', depth: 1, parentToolCallId: 'call-lead' }
      ]
    }
    const moved = relocateNestedTracesToHostRows(conv([lead]), [
      scopedRound({ id: 'row-worker' })
    ])
    expect(moved).toBe(0)
    expect(lead.agentTrace!.map(t => t.id)).toEqual(['inst-worker'])
  })

  it('宿主行缺失时按档案自己的 anchorMessageId 退回归位（老数据）', () => {
    const lead: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      agentTrace: [
        {
          id: 'inst-nested',
          name: '',
          role: '',
          status: 'completed',
          depth: 2,
          // 宿主行 id 在 scoped 行里找不到（老数据把宿主行标成了非 scoped），
          // 但档案锚点仍然指着那条 worker 行。
          parentToolCallId: 'call-missing',
          anchorMessageId: 'row-worker'
        }
      ]
    }
    const workerRow = scopedRound({ id: 'row-worker' })

    const moved = relocateNestedTracesToHostRows(conv([lead]), [workerRow])

    expect(moved).toBe(1)
    expect(lead.agentTrace).toEqual([])
    expect(workerRow.agentTrace!.map(t => t.id)).toEqual(['inst-nested'])
  })

  it('同一档案已在 scoped 行时不重复追加', () => {
    const lead: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      agentTrace: [
        { id: 'inst-nested', name: '', role: '', status: 'completed', depth: 2, parentToolCallId: 'call-nested' }
      ]
    }
    const workerRow = scopedRound({
      id: 'row-worker',
      toolCalls: [{ id: 'call-nested', name: 'run_subagent', arguments: '{}', status: 'success' }],
      agentTrace: [
        { id: 'inst-nested', name: '', role: '', status: 'completed', depth: 2, parentToolCallId: 'call-nested' }
      ]
    })
    relocateNestedTracesToHostRows(conv([lead]), [workerRow])
    expect(workerRow.agentTrace).toHaveLength(1)
    expect(lead.agentTrace).toEqual([])
  })
})

describe('stub 驱逐后不丢子帧', () => {
  function workerTrace(): ChatMessage {
    return {
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 1,
      agentTrace: [
        { id: 'inst-worker', name: '', role: '', status: 'completed', depth: 1, parentToolCallId: 'call-lead' }
      ]
    }
  }

  function workerRound(): ChatMessage {
    return {
      id: 'row-worker',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 2,
      anchorMessageId: 'lead',
      agentInstanceId: 'inst-worker',
      traceId: 'sub_task_1:coder',
      spawnDepth: 1,
      toolCalls: [{ id: 'call-nested', name: 'run_subagent', arguments: '{}', status: 'success' }],
      agentTrace: [
        {
          id: 'inst-nested',
          name: '',
          role: '',
          status: 'completed',
          depth: 2,
          parentToolCallId: 'call-nested',
          parentTraceId: 'inst-worker'
        }
      ]
    }
  }

  function childIds(rows: readonly ChatMessage[]): string[] {
    const lead = workerTrace()
    return buildSubAgentTraceTree({
      leadTraces: lead.agentTrace,
      scopedRows: rows
    })
      .childrenOf('inst-worker')
      .map(t => t.id)
  }

  it('depth-2 档案在父 worker 的 scoped 行上 → 驱逐是可逆的（按落库 payload 重新水合）', () => {
    const store = createConversationScopedStore()
    const persisted = JSON.parse(JSON.stringify(workerRound())) as ChatMessage

    store.ingestRows('c1', [workerRound()])
    expect(childIds(store.listRows('c1'))).toEqual(['inst-nested'])

    // 父帧进 stub：scoped 行被内存驱逐（这正是「加重因素」）。
    store.evictInstance(
      'c1',
      { agentInstanceId: 'inst-worker', anchorMessageId: 'lead' },
      'stub'
    )
    expect(store.listRows('c1')).toEqual([])
    expect(childIds(store.listRows('c1'))).toEqual([])

    // 重新展开 → 按 DB payload 重新水合 → 子帧挂回来（不丢帧）。
    store.ingestRows('c1', [persisted])
    expect(childIds(store.listRows('c1'))).toEqual(['inst-nested'])
  })

  it('depth-2 档案只在 lead 上时，父行驱逐后子帧就彻底消失（反例：这正是 ① 要消灭的形态）', () => {
    const lead = workerTrace()
    lead.agentTrace!.push({
      id: 'inst-nested',
      name: '',
      role: '',
      status: 'completed',
      depth: 2,
      parentToolCallId: 'call-nested',
      parentTraceId: 'inst-worker'
    })
    const rows: ChatMessage[] = []
    expect(childIds(rows)).toEqual([])
    expect(
      buildSubAgentTraceTree({ leadTraces: lead.agentTrace, scopedRows: rows }).all.map(t => t.id)
    ).toEqual(['inst-worker', 'inst-nested'])
  })
})

describe('buildSubAgentRoundsFromScoped', () => {
  function rounds(): ChatMessage[] {
    return [
      {
        id: 'stub',
        role: 'user',
        content: 'Begin. Your assigned task is in the system prompt under **Assigned task**.',
        status: 'done',
        createdAt: 0,
        anchorMessageId: 'lead',
        traceId: 'inst-1',
        agentInstanceId: 'inst-1'
      },
      {
        id: 'r1',
        role: 'assistant',
        content: '第一轮：先看仓库结构',
        status: 'done',
        createdAt: 1,
        anchorMessageId: 'lead',
        traceId: 'inst-1',
        agentInstanceId: 'inst-1',
        toolCalls: [{ id: 't1', name: 'file_list', arguments: '{}', status: 'success' }]
      },
      {
        id: 'r2',
        role: 'assistant',
        content: '',
        status: 'done',
        createdAt: 2,
        anchorMessageId: 'lead',
        traceId: 'inst-1',
        agentInstanceId: 'inst-1',
        toolCalls: [{ id: 't2', name: 'file_read', arguments: '{}', status: 'success' }]
      },
      {
        id: 'r3',
        role: 'assistant',
        content: '最终 handoff：已完成',
        status: 'done',
        createdAt: 3,
        anchorMessageId: 'lead',
        traceId: 'inst-1',
        agentInstanceId: 'inst-1'
      }
    ]
  }

  it('keeps one body per round, each with that round own tools', () => {
    const models = buildSubAgentRoundsFromScoped(rounds(), 'lead', 'inst-1', 'inst-1')
    expect(models.map(r => r.messageId)).toEqual(['r1', 'r2', 'r3'])
    expect(models[0]!.content).toBe('第一轮：先看仓库结构')
    expect(models[0]!.toolCalls.map(tc => tc.id)).toEqual(['t1'])
    expect(models[1]!.content).toBe('')
    expect(models[1]!.toolCalls.map(tc => tc.id)).toEqual(['t2'])
    expect(models[2]!.toolCalls).toEqual([])
    expect(models[2]!.content).toBe('最终 handoff：已完成')
  })

  it('marks the streaming round and drops host stub text', () => {
    const rows = rounds()
    rows[1]!.contentStreaming = true
    rows[0]!.content = 'Begin. Your assigned task is in the system prompt'
    const models = buildSubAgentRoundsFromScoped(rows, 'lead', 'inst-1', 'inst-1')
    expect(models[0]!.contentStreaming).toBe(true)
    expect(models.every(r => !isSubAgentHostStubContent(r.content))).toBe(true)
  })

  it('does not merge rounds into the last one', () => {
    const models = buildSubAgentRoundsFromScoped(rounds(), 'lead', 'inst-1', 'inst-1')
    expect(models).toHaveLength(3)
    expect(models[0]!.content).not.toBe(models[2]!.content)
  })
})
