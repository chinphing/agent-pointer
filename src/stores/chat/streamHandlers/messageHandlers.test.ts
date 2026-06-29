import { describe, expect, it, vi } from 'vitest'
import { handleAssistantJsonPartial, handleDelta, handleMessageStart } from './messageHandlers'
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

  it('handleAssistantJsonPartial preserves real thoughts on empty string', () => {
    const conv = sampleConversation()
    conv.messages.push({
      ...sampleAssistantMessage('a1'),
      thoughts: 'Step result: pass\nNext: click search box'
    })
    const ctx = createMockStreamHandlerContext([conv])
    handleAssistantJsonPartial(ctx, {
      kind: 'assistant_json_partial',
      messageId: 'a1',
      thoughts: ''
    })
    expect(conv.messages[0].thoughts).toBe('Step result: pass\nNext: click search box')
  })
})
