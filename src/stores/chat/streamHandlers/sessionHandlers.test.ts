import { ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { handleDone, handleStreamError } from './sessionHandlers'
import { recordTurnStart, turnElapsedMs } from '../../../lib/turnElapsed'
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

  it('handleDone closes the active turn timing at the done event', () => {
    const conv = sampleConversation()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState: vi.fn(),
      persistAppend: vi.fn()
    })
    recordTurnStart('conv1', 'user-1', 10_000)

    vi.spyOn(Date, 'now').mockReturnValueOnce(75_500)
    handleDone(ctx, { kind: 'done', conversationId: 'conv1' })

    expect(turnElapsedMs('conv1', 'user-1')).toBe(65_500)
    vi.restoreAllMocks()
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
      clearRunState: vi.fn()
    })

    handleStreamError(ctx, {
      kind: 'error',
      conversationId: 'conv1',
      message: 'network failed'
    })
    expect(conv.messages).toHaveLength(1)
    expect(conv.messages[0].status).toBe('error')
    expect(conv.messages[0].errorMessage).toBe('network failed')
    expect(markMetaDirty).toHaveBeenCalledWith('conv1')
  })

  it('handleStreamError marks user cancel as cancelled, not error', () => {
    const conv = sampleConversation()
    conv.messages.push({
      id: 'a1',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: Date.now(),
      toolCalls: [{ id: 't1', name: 'terminal', status: 'running', arguments: '{}' }]
    })
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      markMetaDirty: vi.fn(),
      clearRunState: vi.fn()
    })

    handleStreamError(ctx, {
      kind: 'error',
      conversationId: 'conv1',
      messageId: 'a1',
      message: 'cancelled'
    })

    expect(conv.messages).toHaveLength(1)
    expect(conv.messages[0].status).toBe('cancelled')
    expect(conv.messages[0].errorMessage).toBe('已停止生成')
  })

  it('handleStreamError keeps empty streaming shell as cancelled on stop', () => {
    const conv = sampleConversation()
    conv.messages.push({
      id: 'a1',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: Date.now(),
      toolCalls: []
    })
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      markMetaDirty: vi.fn(),
      clearRunState: vi.fn()
    })

    handleStreamError(ctx, {
      kind: 'error',
      conversationId: 'conv1',
      messageId: 'a1',
      message: '已停止生成'
    })

    expect(conv.messages).toHaveLength(1)
    expect(conv.messages[0].status).toBe('cancelled')
    expect(conv.messages[0].errorMessage).toBe('已停止生成')
  })
  it('handleStreamError attaches session-level error to event conversation, not current open one', () => {
    const convA = sampleConversation('convA')
    const convB = sampleConversation('convB')
    const clearRunState = vi.fn()
    const clearAllRunStates = vi.fn()
    const markMetaDirty = vi.fn()
    const ctx = createMockStreamHandlerContext([convA, convB], {
      currentId: ref('convB'),
      clearRunState,
      clearAllRunStates,
      markMetaDirty
    })

    handleStreamError(ctx, {
      kind: 'error',
      conversationId: 'convA',
      message: 'run failed in background'
    })

    expect(convA.messages).toHaveLength(1)
    expect(convA.messages[0].status).toBe('error')
    expect(convA.messages[0].errorMessage).toBe('run failed in background')
    expect(convB.messages).toHaveLength(0)
    expect(clearRunState).toHaveBeenCalledWith('convA')
    expect(clearAllRunStates).not.toHaveBeenCalled()
    expect(markMetaDirty).toHaveBeenCalledWith('convA')
  })
})
