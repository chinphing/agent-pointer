import { ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { handleDone, handleStreamError } from './sessionHandlers'
import { createMockStreamHandlerContext, sampleConversation } from './testUtils'

describe('sessionHandlers', () => {
  it('handleDone clears run state and persists meta', () => {
    const conv = sampleConversation()
    conv.toolRoundsUsed = 2
    const clearRunState = vi.fn()
    const persistMeta = vi.fn()
    const persistAppend = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState,
      persistMeta,
      persistAppend
    })

    handleDone(ctx, {
      kind: 'done',
      conversationId: 'conv1',
      toolRoundsUsedTotal: 5
    })
    expect(clearRunState).toHaveBeenCalledWith('conv1')
    expect(conv.toolRoundsUsed).toBe(5)
    expect(persistAppend).toHaveBeenCalledWith('conv1')
    expect(persistMeta).toHaveBeenCalledOnce()
  })

  it('handleStreamError sets assistant error row', () => {
    const conv = sampleConversation()
    const persistMeta = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      persistMeta,
      clearAllRunStates: vi.fn()
    })

    handleStreamError(ctx, {
      kind: 'error',
      message: 'network failed'
    })
    expect(conv.messages).toHaveLength(1)
    expect(conv.messages[0].status).toBe('error')
    expect(conv.messages[0].errorMessage).toBe('network failed')
    expect(persistMeta).toHaveBeenCalledOnce()
  })
})
