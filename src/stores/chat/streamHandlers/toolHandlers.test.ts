import { describe, expect, it, vi } from 'vitest'
import { handleToolCallStart, handleToolCallStatus } from './toolHandlers'
import { createMockStreamHandlerContext, sampleAssistantMessage, sampleConversation } from './testUtils'

describe('toolHandlers', () => {
  it('handleToolCallStart upserts tool call on assistant message', () => {
    const conv = sampleConversation()
    conv.messages.push(sampleAssistantMessage('a1'))
    const ctx = createMockStreamHandlerContext([conv])
    handleToolCallStart(ctx, {
      kind: 'tool_call_start',
      messageId: 'a1',
      toolCall: {
        id: 'tc1',
        name: 'file_read',
        arguments: '{}',
        status: 'running'
      }
    })
    expect(conv.messages[0].toolCalls).toHaveLength(1)
    expect(conv.messages[0].toolCalls?.[0].name).toBe('file_read')
  })

  it('handleToolCallStart merges duplicate tool_call_id', () => {
    const conv = sampleConversation()
    const msg = sampleAssistantMessage('a1')
    msg.toolCalls = [
      { id: 'tc1', name: 'file_read', arguments: '{}', status: 'running' }
    ]
    conv.messages.push(msg)
    const ctx = createMockStreamHandlerContext([conv])
    handleToolCallStart(ctx, {
      kind: 'tool_call_start',
      messageId: 'a1',
      toolCall: {
        id: 'tc1',
        name: 'file_read',
        arguments: '{}',
        status: 'running',
        displayLabel: '读文件'
      }
    })
    expect(conv.messages[0].toolCalls).toHaveLength(1)
    expect(conv.messages[0].toolCalls?.[0].displayLabel).toBe('读文件')
  })

  it('handleToolCallStart refreshes arguments without clobbering success', () => {
    const conv = sampleConversation()
    const msg = sampleAssistantMessage('a1')
    msg.toolCalls = [
      {
        id: 'tc1',
        name: 'file_edit',
        arguments: '{"path":"a.py"}extra}',
        status: 'success',
        result: '{"success":true}',
        durationMs: 12
      }
    ]
    conv.messages.push(msg)
    const ctx = createMockStreamHandlerContext([conv])
    handleToolCallStart(ctx, {
      kind: 'tool_call_start',
      messageId: 'a1',
      toolCall: {
        id: 'tc1',
        name: 'file_edit',
        arguments: '{"path":"a.py","oldString":"x","newString":"y"}',
        status: 'pending'
      }
    })
    const tc = conv.messages[0].toolCalls?.[0]
    expect(tc?.arguments).toBe('{"path":"a.py","oldString":"x","newString":"y"}')
    expect(tc?.status).toBe('success')
    expect(tc?.result).toBe('{"success":true}')
    expect(tc?.durationMs).toBe(12)
  })

  it('handleToolCallStatus forwards terminal hook', () => {
    const conv = sampleConversation()
    const msg = sampleAssistantMessage('a1')
    msg.toolCalls = [
      { id: 'tc1', name: 'terminal', arguments: '{}', status: 'running' }
    ]
    conv.messages.push(msg)
    const handleTerminalToolCallStatus = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], { handleTerminalToolCallStatus })
    handleToolCallStatus(ctx, {
      kind: 'tool_call_status',
      messageId: 'a1',
      toolCallId: 'tc1',
      status: 'success',
      result: 'ok'
    })
    expect(conv.messages[0].toolCalls?.[0].status).toBe('success')
    expect(handleTerminalToolCallStatus).toHaveBeenCalledWith('a1', 'tc1', 'success', undefined, undefined)
  })

  it('clears background occupancy when the last background host finishes', () => {
    const conv = sampleConversation()
    const msg = sampleAssistantMessage('a1')
    msg.toolCalls = [
      {
        id: 'bg1',
        name: 'run_subagent',
        arguments: JSON.stringify({ agentId: 'explore', goal: 'map', background: true }),
        status: 'running',
        result: '{"jobId":"job_1","status":"running","kind":"subagent"}'
      }
    ]
    conv.messages.push(msg)
    const clearBackgroundJobsIfNoneLive = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], { clearBackgroundJobsIfNoneLive })
    handleToolCallStatus(ctx, {
      kind: 'tool_call_status',
      messageId: 'a1',
      toolCallId: 'bg1',
      status: 'success',
      result: '{"jobId":"job_1","status":"completed","kind":"subagent"}'
    })
    expect(clearBackgroundJobsIfNoneLive).toHaveBeenCalledWith('conv1')
  })
})
