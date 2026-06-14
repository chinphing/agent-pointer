import { describe, expect, it, vi } from 'vitest'
import { handleContextTrimApplied } from './contextHandlers'
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
    const persistMeta = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], { persistMeta })
    handleContextTrimApplied(ctx, {
      kind: 'context_trim_applied',
      conversationId: 'conv1',
      excludedMessageIds: ['m1']
    })
    expect(conv.messages[0].contextState?.excludedReason).toBe('task_board_trim')
    expect(persistMeta).toHaveBeenCalledOnce()
  })
})
