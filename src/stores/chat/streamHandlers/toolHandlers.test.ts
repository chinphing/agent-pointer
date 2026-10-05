import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  clearStreamDeltaBuffers,
  enqueueContentDelta,
  setContentDeltaApplyHandler,
  setScopedMessageIdPredicate,
  SLOW_LANE_BATCH_MS
} from '../../../lib/reasoningDeltaBatch'
import { handleToolCallStart, handleToolCallStatus } from './toolHandlers'
import { createMockStreamHandlerContext, sampleAssistantMessage, sampleConversation } from './testUtils'

afterEach(() => {
  clearStreamDeltaBuffers()
  setContentDeltaApplyHandler(null)
  setScopedMessageIdPredicate(null)
  vi.useRealTimers()
})

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

  it('does not infer occupancy from a finished background host row', () => {
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
    const ctx = createMockStreamHandlerContext([conv])
    handleToolCallStatus(ctx, {
      kind: 'tool_call_status',
      messageId: 'a1',
      toolCallId: 'bg1',
      status: 'success',
      result: '{"jobId":"job_1","status":"completed","kind":"subagent"}'
    })
    expect(conv.messages[0].toolCalls?.[0].status).toBe('success')
  })

  // --- Escape valve 2: ask_user / approval --------------------------------

  it('an ask_user start flushes that row\'s slow-lane text immediately', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    const conv = sampleConversation()
    conv.messages.push(sampleAssistantMessage('row-1'))
    const ctx = createMockStreamHandlerContext([conv])

    enqueueContentDelta('row-1', '等待确认前的正文')
    expect(apply).not.toHaveBeenCalled()

    handleToolCallStart(ctx, {
      kind: 'tool_call_start',
      messageId: 'row-1',
      toolCall: {
        id: 'tc-ask',
        name: 'ask_user',
        arguments: '{"question":"继续吗","options":[]}',
        status: 'pending'
      }
    })

    // The card is on screen — the text must already be there, not 1s later.
    expect(apply).toHaveBeenCalledWith('row-1', undefined, undefined, '等待确认前的正文')
    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(1)
  })

  it('an approval start flushes that row\'s slow-lane text immediately', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    const conv = sampleConversation()
    conv.messages.push(sampleAssistantMessage('row-2'))
    const ctx = createMockStreamHandlerContext([conv])

    enqueueContentDelta('row-2', '即将请求授权')

    handleToolCallStart(ctx, {
      kind: 'tool_call_start',
      messageId: 'row-2',
      toolCall: {
        id: 'tc-approve',
        name: 'terminal',
        arguments: '{"command":"rm -rf /tmp/x"}',
        status: 'pending_approval'
      }
    })

    expect(apply).toHaveBeenCalledWith('row-2', undefined, undefined, '即将请求授权')
  })

  it('a regular tool start leaves the slow lane batched', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    setScopedMessageIdPredicate(id => id.startsWith('row-'))

    const conv = sampleConversation()
    conv.messages.push(sampleAssistantMessage('row-3'))
    const ctx = createMockStreamHandlerContext([conv])

    enqueueContentDelta('row-3', '普通工具前的正文')

    handleToolCallStart(ctx, {
      kind: 'tool_call_start',
      messageId: 'row-3',
      toolCall: { id: 'tc-read', name: 'file_read', arguments: '{}', status: 'running' }
    })

    expect(apply).not.toHaveBeenCalled()
    vi.advanceTimersByTime(SLOW_LANE_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(1)
  })
})
