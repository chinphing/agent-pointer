import { describe, expect, it } from 'vitest'
import type { Conversation } from '../types/chat'
import {
  buildSubAgentBodyModelsFromScoped,
  computeSubAgentStatsFromMessages,
  ensureScopedChildMessage,
  isSubAgentHostStubContent,
  rehydrateAgentTracesFromScopedMessages,
  scopedAssistantMessagesForTrace,
  scopedMessagesForTrace
} from './subAgentMessages'
import { formatSubAgentSummaryLine } from './subAgentStats'

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
