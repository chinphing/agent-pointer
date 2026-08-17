import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  clearContentDeltaBuffer,
  setContentDeltaApplyHandler
} from '../../../lib/reasoningDeltaBatch'
import {
  handleAssistantJsonPartial,
  handleDelta,
  handleRawContentDelta,
  handleMessageEnd,
  handleMessageStart
} from './messageHandlers'
import { createMockStreamHandlerContext, sampleAssistantMessage, sampleConversation } from './testUtils'
import { PLANNER_PHASE_THOUGHTS } from '../../../lib/plannerPhase'

afterEach(() => {
  clearContentDeltaBuffer()
  setContentDeltaApplyHandler(null)
})

describe('messageHandlers', () => {
  it('handleMessageStart appends streaming assistant row', () => {
    const conv = sampleConversation()
    const ctx = createMockStreamHandlerContext([conv])
    handleMessageStart(ctx, {
      kind: 'message_start',
      conversationId: 'conv1',
      messageId: 'a_new'
    })
    expect(conv.messages).toHaveLength(1)
    expect(conv.messages[0]).toMatchObject({
      id: 'a_new',
      role: 'assistant',
      status: 'streaming',
      contentStreaming: true
    })
  })

  it('handleDelta queues streamed text for the content batch', () => {
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)
    handleDelta(createMockStreamHandlerContext([]), {
      kind: 'delta',
      messageId: 'a1',
      text: 'hello'
    })
    clearContentDeltaBuffer()
    expect(apply).not.toHaveBeenCalled()
  })

  it('handleMessageStart patches run state when shell exists', () => {
    const conv = sampleConversation()
    const patchRunState = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], { patchRunState })
    handleMessageStart(ctx, {
      kind: 'message_start',
      conversationId: 'conv1',
      messageId: 'a1'
    })
    expect(patchRunState).toHaveBeenCalledWith('conv1', {
      generating: true,
      activeMessageId: 'a1'
    })
  })

  it('handleMessageStart skips run state when shell is missing', () => {
    const patchRunState = vi.fn()
    const ctx = createMockStreamHandlerContext([], { patchRunState })
    handleMessageStart(ctx, {
      kind: 'message_start',
      conversationId: 'webhook:github',
      messageId: 'a1'
    })
    expect(patchRunState).not.toHaveBeenCalled()
  })

  it('handleAssistantJsonPartial clears planner placeholder on empty string', () => {
    const conv = sampleConversation()
    conv.messages.push({
      ...sampleAssistantMessage('a1'),
      thoughts: PLANNER_PHASE_THOUGHTS
    })
    const ctx = createMockStreamHandlerContext([conv])
    handleAssistantJsonPartial(ctx, {
      kind: 'assistant_json_partial',
      messageId: 'a1',
      thoughts: ''
    })
    expect(conv.messages[0].thoughts).toBeUndefined()
  })

  it('handleMessageEnd applies reply attachments on lead assistant message', () => {
    const conv = sampleConversation()
    conv.messages.push({
      ...sampleAssistantMessage('a1'),
      content: '这张就是你刚发的那张图片，直接给你：',
      contentStreaming: true
    })
    const ctx = createMockStreamHandlerContext([conv])
    handleMessageEnd(ctx, {
      kind: 'message_end',
      messageId: 'a1',
      content: '这张就是你刚发的那张图片，直接给你：',
      rawContent:
        '这张就是你刚发的那张图片，直接给你：\n\nMEDIA:/tmp/logo2.png',
      attachments: [
        {
          id: 'reply-media-1',
          kind: 'image',
          mimeType: 'image/png',
          fileName: 'logo2.png',
          sizeBytes: 0,
          localAbsPath: '/tmp/logo2.png'
        }
      ]
    })
    expect(conv.messages[0].attachments).toHaveLength(1)
    expect(conv.messages[0].attachments?.[0]?.fileName).toBe('logo2.png')
    expect(conv.messages[0].status).toBe('done')
    expect(conv.messages[0].contentStreaming).toBe(false)
  })

  it('handleMessageEnd does not overwrite cancelled status after user stop', () => {
    const conv = sampleConversation()
    conv.messages.push({
      ...sampleAssistantMessage('a1'),
      status: 'cancelled',
      errorMessage: '已停止生成',
      contentStreaming: false,
      toolCalls: [{ id: 't1', name: 'terminal', status: 'failed', arguments: '{}' }]
    })
    const ctx = createMockStreamHandlerContext([conv], {
      isConversationGenerating: () => false
    })
    handleMessageEnd(ctx, {
      kind: 'message_end',
      messageId: 'a1',
      content: ''
    })
    expect(conv.messages[0].status).toBe('cancelled')
    expect(conv.messages[0].errorMessage).toBe('已停止生成')
  })

  it('handleMessageEnd for scoped sub-agent clears contentStreaming only', () => {
    const conv = sampleConversation()
    conv.messages.push({
      ...sampleAssistantMessage('a1'),
      content: 'delegating…',
      contentStreaming: false,
      toolCalls: [{ id: 'rs1', name: 'run_subagent', status: 'running', arguments: '{}' }]
    })
    conv.messages.push({
      ...sampleAssistantMessage('sub-round-1'),
      content: 'sub done',
      contentStreaming: true
    })
    const clearRunState = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      isConversationGenerating: () => true,
      clearRunState
    })
    handleMessageEnd(ctx, {
      kind: 'message_end',
      messageId: 'a1',
      scopedMessageId: 'sub-round-1',
      traceId: 'trace-1',
      content: 'sub done'
    })
    expect(conv.messages.find(m => m.id === 'sub-round-1')?.contentStreaming).toBe(false)
    expect(clearRunState).not.toHaveBeenCalled()
  })

  describe('rawContent capture gating (rawContentViewEnabled)', () => {
    it('handleRawContentDelta accumulates while capture is enabled', () => {
      const conv = sampleConversation()
      conv.messages.push(sampleAssistantMessage('a1'))
      const ctx = createMockStreamHandlerContext([conv], {
        rawContentCaptureEnabled: () => true
      })
      handleRawContentDelta(ctx, { kind: 'raw_content_delta', messageId: 'a1', text: 'part-1' })
      handleRawContentDelta(ctx, { kind: 'raw_content_delta', messageId: 'a1', text: 'part-2' })
      expect(conv.messages[0].rawContent).toBe('part-1part-2')
    })

    it('handleRawContentDelta skips capture while disabled', () => {
      const conv = sampleConversation()
      conv.messages.push(sampleAssistantMessage('a1'))
      const ctx = createMockStreamHandlerContext([conv], {
        rawContentCaptureEnabled: () => false
      })
      handleRawContentDelta(ctx, { kind: 'raw_content_delta', messageId: 'a1', text: 'part-1' })
      expect(conv.messages[0].rawContent).toBeUndefined()
    })

    it('handleMessageEnd applies rawContent only while capture is enabled', () => {
      const conv = sampleConversation()
      conv.messages.push({ ...sampleAssistantMessage('a1'), content: 'done' })
      const ctx = createMockStreamHandlerContext([conv], {
        rawContentCaptureEnabled: () => false
      })
      handleMessageEnd(ctx, {
        kind: 'message_end',
        messageId: 'a1',
        content: 'done',
        rawContent: 'done\n\nMEDIA:/tmp/x.png'
      })
      expect(conv.messages[0].rawContent).toBeUndefined()
      expect(conv.messages[0].content).toBe('done')
    })

    it('handleMessageEnd applies rawContent when capture is enabled', () => {
      const conv = sampleConversation()
      conv.messages.push({ ...sampleAssistantMessage('a1'), content: 'done' })
      const ctx = createMockStreamHandlerContext([conv], {
        rawContentCaptureEnabled: () => true
      })
      handleMessageEnd(ctx, {
        kind: 'message_end',
        messageId: 'a1',
        content: 'done',
        rawContent: 'done\n\nMEDIA:/tmp/x.png'
      })
      expect(conv.messages[0].rawContent).toBe('done\n\nMEDIA:/tmp/x.png')
    })

    it('handleRawContentDelta skips legacy session capture while disabled', () => {
      const conv = sampleConversation()
      conv.messages.push(sampleAssistantMessage('a1'))
      const ctx = createMockStreamHandlerContext([conv], {
        rawContentCaptureEnabled: () => false
      })
      handleRawContentDelta(ctx, {
        kind: 'raw_content_delta',
        messageId: 'a1',
        text: 'sub-part',
        traceId: 'trace-1'
      })
      expect(conv.messages[0].agentTrace?.[0]?.session?.rawContent).toBeUndefined()
    })

    it('handleRawContentDelta captures legacy session while enabled', () => {
      const conv = sampleConversation()
      conv.messages.push(sampleAssistantMessage('a1'))
      const ctx = createMockStreamHandlerContext([conv], {
        rawContentCaptureEnabled: () => true
      })
      handleRawContentDelta(ctx, {
        kind: 'raw_content_delta',
        messageId: 'a1',
        text: 'sub-part',
        traceId: 'trace-1'
      })
      expect(conv.messages[0].agentTrace?.[0]?.session?.rawContent).toBe('sub-part')
    })

    it('handleMessageEnd skips scoped sub-message rawContent while disabled', () => {
      const conv = sampleConversation()
      conv.messages.push({
        ...sampleAssistantMessage('a1'),
        content: 'delegating…',
        contentStreaming: false,
        toolCalls: [{ id: 'rs1', name: 'run_subagent', status: 'running', arguments: '{}' }]
      })
      conv.messages.push({
        ...sampleAssistantMessage('sub-round-1'),
        content: 'sub done',
        contentStreaming: true
      })
      const ctx = createMockStreamHandlerContext([conv], {
        isConversationGenerating: () => true,
        rawContentCaptureEnabled: () => false
      })
      handleMessageEnd(ctx, {
        kind: 'message_end',
        messageId: 'a1',
        scopedMessageId: 'sub-round-1',
        traceId: 'trace-1',
        content: 'sub done',
        rawContent: 'sub done\n\nMEDIA:/tmp/y.png'
      })
      expect(conv.messages.find(m => m.id === 'sub-round-1')?.rawContent).toBeUndefined()
    })
  })
})
