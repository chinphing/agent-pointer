import { describe, expect, it, vi } from 'vitest'
import { handleDelta, handleMessageStart } from './messageHandlers'
import { createMockStreamHandlerContext, sampleAssistantMessage, sampleConversation } from './testUtils'

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

  it('handleMessageStart patches run state', () => {
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
})
