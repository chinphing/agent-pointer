import { describe, expect, it, vi } from 'vitest'
import {
  handleContextCompressionApplied,
  handleContextCompressionStarted,
  handleContextTrimApplied
} from './contextHandlers'
import { createMockStreamHandlerContext, sampleConversation } from './testUtils'

describe('contextHandlers', () => {
  it('handleContextTrimApplied marks excluded ids and persists meta', () => {
    const conv = sampleConversation()
    conv.messages.push({
      id: 'm1',
      role: 'user',
      content: 'old',
      status: 'done',
      createdAt: 0
    })
    const markMetaDirty = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], { markMetaDirty })
    handleContextTrimApplied(ctx, {
      kind: 'context_trim_applied',
      conversationId: 'conv1',
      excludedMessageIds: ['m1']
    })
    expect(conv.messages[0].contextState?.excludedReason).toBe('task_board_trim')
    expect(markMetaDirty).toHaveBeenCalledWith('conv1')
  })

  it('handleContextCompressionStarted sets run-state marker', () => {
    const conv = sampleConversation()
    const patchRunState = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], { patchRunState })
    handleContextCompressionStarted(ctx, {
      kind: 'context_compression_started',
      conversationId: 'conv1',
      scope: 'main'
    })
    expect(patchRunState).toHaveBeenCalledWith(
      'conv1',
      expect.objectContaining({
        contextCompressing: expect.objectContaining({ scope: 'main' })
      })
    )
  })

  it('handleContextCompressionApplied clears compressing marker', () => {
    const conv = sampleConversation()
    conv.messages.push({
      id: 'anchor',
      role: 'user',
      content: 'hi',
      status: 'done',
      createdAt: 1
    })
    const patchRunState = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], { patchRunState })
    handleContextCompressionApplied(ctx, {
      kind: 'context_compression_applied',
      conversationId: 'conv1',
      excludedMessageIds: [],
      insertBeforeMessageId: 'anchor',
      summaryMessage: {
        id: 'sum1',
        role: 'user',
        content: '[Conversation summary (auto-compression)]\nbody',
        status: 'done',
        createdAt: 0
      },
      compression: {
        reason: 'budget',
        messagesBefore: 10,
        messagesAfter: 4,
        droppedCount: 6,
        keepRecentUserTurns: 2,
        scope: 'main'
      }
    })
    expect(patchRunState).toHaveBeenCalledWith('conv1', { contextCompressing: null })
  })
})
