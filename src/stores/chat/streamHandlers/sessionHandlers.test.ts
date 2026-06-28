import { ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { handleDone, handleStreamError } from './sessionHandlers'
import { createMockStreamHandlerContext, sampleConversation } from './testUtils'

describe('sessionHandlers', () => {
  it('handleDone clears run state and persists meta', () => {
    const conv = sampleConversation()
    conv.toolRoundsUsed = 2
    const clearRunState = vi.fn()
    const markMetaDirty = vi.fn()
    const persistAppend = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState,
      markMetaDirty,
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
    expect(markMetaDirty).toHaveBeenCalledWith('conv1')
  })

  it('handleDone clears only the finished conversation when another is still generating', () => {
    const convA = sampleConversation('convA')
    const convB = sampleConversation('convB')
    const clearRunState = vi.fn()
    const isConversationGenerating = vi.fn((id: string) => id === 'convB')
    const ctx = createMockStreamHandlerContext([convA, convB], {
      currentId: ref('convB'),
      clearRunState,
      isConversationGenerating,
      persistMeta: vi.fn(),
      persistAppend: vi.fn()
    })

    handleDone(ctx, {
      kind: 'done',
      conversationId: 'convA',
      toolRoundsUsedTotal: 5
    })

    expect(clearRunState).toHaveBeenCalledTimes(1)
    expect(clearRunState).toHaveBeenCalledWith('convA')
    expect(clearRunState).not.toHaveBeenCalledWith('convB')
  })

  it('handleStreamError sets assistant error row', () => {
    const conv = sampleConversation()
    const markMetaDirty = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      markMetaDirty,
      clearAllRunStates: vi.fn()
    })

    handleStreamError(ctx, {
      kind: 'error',
      message: 'network failed'
    })
    expect(conv.messages).toHaveLength(1)
    expect(conv.messages[0].status).toBe('error')
    expect(conv.messages[0].errorMessage).toBe('network failed')
    expect(markMetaDirty).toHaveBeenCalledWith('conv1')
  })
})
