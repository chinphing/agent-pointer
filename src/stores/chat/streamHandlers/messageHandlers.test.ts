import { describe, expect, it, vi } from 'vitest'
import {
  handleAssistantJsonPartial,
  handleDelta,
  handleMessageEnd,
  handleMessageStart
} from './messageHandlers'
import { createMockStreamHandlerContext, sampleAssistantMessage, sampleConversation } from './testUtils'
import { PLANNER_PHASE_THOUGHTS } from '../../../lib/plannerPhase'

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

  it('handleDelta appends streamed text', () => {
    const conv = sampleConversation()
    conv.messages.push(sampleAssistantMessage('a1'))
    const ctx = createMockStreamHandlerContext([conv])
    handleDelta(ctx, { kind: 'delta', messageId: 'a1', text: 'hello' })
    expect(conv.messages[0].content).toBe('hello')
    expect(conv.messages[0].contentStreaming).toBe(true)
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
})
