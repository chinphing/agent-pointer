import { ref } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { handleBackgroundJobs, handleDone, handleStreamError } from './sessionHandlers'
import { recordTurnStart, turnElapsedMs, hasActiveTurn } from '../../../lib/turnElapsed'
import { applyUiLocale } from '../../../lib/uiLocale'
import { createMockStreamHandlerContext, sampleConversation } from './testUtils'

const playTaskCompleteSoundIfEnabled = vi.hoisted(() => vi.fn())
const disarmTaskCompleteAudio = vi.hoisted(() => vi.fn())

vi.mock('../../../lib/taskCompleteSound', () => ({
  playTaskCompleteSoundIfEnabled,
  disarmTaskCompleteAudio
}))

// Cancellation copy comes from the i18n catalogue, and the UI locale is
// resolved from the host language — pin it so the assertions do not depend on
// the machine locale (jsdom's `navigator.language` follows LC_ALL / LANG).
beforeEach(() => {
  applyUiLocale('zh-CN')
})

describe('sessionHandlers', () => {
  beforeEach(() => {
    playTaskCompleteSoundIfEnabled.mockClear()
    disarmTaskCompleteAudio.mockClear()
  })
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
      persistAppend,
      isConversationGenerating: (id: string) => id === 'conv1'
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

  it('handleDone prefers backend run_chat timestamps over local timing', () => {
    const conv = sampleConversation()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState: vi.fn(),
      persistAppend: vi.fn()
    })
    recordTurnStart('conv1', 'user-1', 10_000)

    handleDone(ctx, {
      kind: 'done',
      conversationId: 'conv1',
      startedAtMs: 20_000,
      finishedAtMs: 50_000
    })

    expect(turnElapsedMs('conv1', 'user-1')).toBe(30_000)
  })

  it('handleDone ignores stale Done after interrupt when a newer turn is active', () => {
    const conv = sampleConversation()
    const clearRunState = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState,
      persistAppend: vi.fn(),
      isConversationGenerating: () => true,
      consumeStaleDoneAfterInterrupt: () => true
    })
    recordTurnStart('conv1', 'user-force-sent', 50_000)

    handleDone(ctx, { kind: 'done', conversationId: 'conv1' })

    expect(clearRunState).not.toHaveBeenCalled()
    expect(hasActiveTurn('conv1')).toBe(true)
    expect(turnElapsedMs('conv1', 'user-force-sent')).toBeNull()
  })

  it('handleStreamError ignores stale cancelled Error so the next turn keeps timing', () => {
    const conv = sampleConversation()
    conv.messages.push({
      id: 'a-old',
      role: 'assistant',
      content: '',
      status: 'cancelled',
      createdAt: Date.now()
    })
    const clearRunState = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState,
      markMetaDirty: vi.fn(),
      isStaleStreamAfterInterrupt: () => true
    })
    recordTurnStart('conv1', 'user-force-sent', 50_000)

    handleStreamError(ctx, {
      kind: 'error',
      conversationId: 'conv1',
      messageId: 'a-old',
      message: 'cancelled'
    })

    expect(clearRunState).not.toHaveBeenCalled()
    expect(hasActiveTurn('conv1')).toBe(true)
    expect(turnElapsedMs('conv1', 'user-force-sent')).toBeNull()
    expect(disarmTaskCompleteAudio).not.toHaveBeenCalled()
  })

  it('handleDone falls back to local timing when the backend span is invalid', () => {
    const conv = sampleConversation()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState: vi.fn(),
      persistAppend: vi.fn()
    })
    recordTurnStart('conv1', 'user-1', 10_000)

    handleDone(ctx, {
      kind: 'done',
      conversationId: 'conv1',
      startedAtMs: 50_000,
      finishedAtMs: 20_000
    })

    expect(hasActiveTurn('conv1')).toBe(false)
    expect(turnElapsedMs('conv1', 'user-1')).toBeGreaterThan(0)
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

  it('handleDone marks background conversation awaiting view', () => {
    const convA = sampleConversation('convA')
    const markConversationAwaitingView = vi.fn()
    const ctx = createMockStreamHandlerContext([convA], {
      currentId: ref('convB'),
      clearRunState: vi.fn(),
      persistAppend: vi.fn(),
      isConversationGenerating: (id: string) => id === 'convA',
      markConversationAwaitingView
    })

    handleDone(ctx, { kind: 'done', conversationId: 'convA' })

    expect(markConversationAwaitingView).toHaveBeenCalledWith('convA')
  })

  it('handleDone does not mark awaiting view for the focused conversation', () => {
    const conv = sampleConversation()
    const markConversationAwaitingView = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState: vi.fn(),
      persistAppend: vi.fn(),
      isConversationGenerating: () => true,
      markConversationAwaitingView
    })

    handleDone(ctx, { kind: 'done', conversationId: 'conv1' })

    expect(markConversationAwaitingView).not.toHaveBeenCalled()
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
  it('handleStreamError does not push a second row when messageId already marked error', () => {
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
      message: 'HTTP 400 Bad Request: tool choice'
    })
    expect(conv.messages).toHaveLength(1)
    expect(conv.messages[0].status).toBe('error')
    expect(conv.messages[0].errorMessage).toBe('HTTP 400 Bad Request: tool choice')
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

  it('handleDone plays chime once with turn id when generating', () => {
    const conv = sampleConversation()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState: vi.fn(),
      persistAppend: vi.fn(),
      isConversationGenerating: () => true
    })
    recordTurnStart('conv1', 'user-turn-1', 10_000)

    handleDone(ctx, { kind: 'done', conversationId: 'conv1' })

    expect(playTaskCompleteSoundIfEnabled).toHaveBeenCalledTimes(1)
    expect(playTaskCompleteSoundIfEnabled).toHaveBeenCalledWith('conv1', 'user-turn-1')
  })

  it('handleStreamError closes active turn so trailing Done does not chime', () => {
    const conv = sampleConversation()
    conv.messages.push({
      id: 'a1',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: Date.now()
    })
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      markMetaDirty: vi.fn(),
      clearRunState: vi.fn(),
      isConversationGenerating: () => false
    })
    recordTurnStart('conv1', 'user-err', 10_000)

    handleStreamError(ctx, {
      kind: 'error',
      conversationId: 'conv1',
      messageId: 'a1',
      message: 'boom'
    })
    expect(hasActiveTurn('conv1')).toBe(false)
    expect(disarmTaskCompleteAudio).toHaveBeenCalled()

    handleDone(ctx, { kind: 'done', conversationId: 'conv1' })
    expect(playTaskCompleteSoundIfEnabled).not.toHaveBeenCalled()
  })

  it('handleDone does not chime twice for duplicate Done on the same turn', () => {
    const conv = sampleConversation()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState: vi.fn(),
      persistAppend: vi.fn(),
      isConversationGenerating: vi
        .fn()
        .mockReturnValueOnce(true)
        .mockReturnValue(false)
    })
    recordTurnStart('conv1', 'user-dup', 10_000)

    handleDone(ctx, { kind: 'done', conversationId: 'conv1' })
    handleDone(ctx, { kind: 'done', conversationId: 'conv1' })

    // Second Done has no generating / active turn → handler skips before play.
    expect(playTaskCompleteSoundIfEnabled).toHaveBeenCalledTimes(1)
  })

  it('handleDone skips chime while background jobs are still running', () => {
    const conv = sampleConversation()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState: vi.fn(),
      persistAppend: vi.fn(),
      isConversationGenerating: () => true,
      hasBackgroundJobs: (id: string) => id === 'conv1'
    })
    recordTurnStart('conv1', 'user-bg', 10_000)

    handleDone(ctx, { kind: 'done', conversationId: 'conv1' })
    expect(playTaskCompleteSoundIfEnabled).not.toHaveBeenCalled()
  })

  it('handleDone applies occupancy from Done so the stop button can clear', () => {
    const conv = sampleConversation()
    const setBackgroundJobCount = vi.fn()
    const reconcileBackgroundHostsWhenOccupancyEmpty = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      currentId: ref('conv1'),
      clearRunState: vi.fn(),
      persistAppend: vi.fn(),
      isConversationGenerating: () => true,
      hasBackgroundJobs: () => false,
      setBackgroundJobCount,
      reconcileBackgroundHostsWhenOccupancyEmpty
    })
    handleDone(ctx, {
      kind: 'done',
      conversationId: 'conv1',
      backgroundRunningCount: 0
    })
    expect(setBackgroundJobCount).toHaveBeenCalledWith('conv1', 0)
    expect(reconcileBackgroundHostsWhenOccupancyEmpty).toHaveBeenCalledWith('conv1')
  })

  it('handleBackgroundJobs updates occupancy', () => {
    const conv = sampleConversation()
    const setBackgroundJobCount = vi.fn()
    const reconcileBackgroundHostsWhenOccupancyEmpty = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      setBackgroundJobCount,
      reconcileBackgroundHostsWhenOccupancyEmpty
    })
    handleBackgroundJobs(ctx, {
      kind: 'background_jobs',
      conversationId: 'conv1',
      runningCount: 2
    })
    expect(setBackgroundJobCount).toHaveBeenCalledWith('conv1', 2, undefined)
    expect(reconcileBackgroundHostsWhenOccupancyEmpty).not.toHaveBeenCalled()
  })

  it('handleBackgroundJobs hydrates leftover hosts when occupancy hits 0', () => {
    const conv = sampleConversation()
    const setBackgroundJobCount = vi.fn()
    const reconcileBackgroundHostsWhenOccupancyEmpty = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], {
      setBackgroundJobCount,
      reconcileBackgroundHostsWhenOccupancyEmpty
    })
    handleBackgroundJobs(ctx, {
      kind: 'background_jobs',
      conversationId: 'conv1',
      runningCount: 0
    })
    expect(setBackgroundJobCount).toHaveBeenCalledWith('conv1', 0, [])
    expect(reconcileBackgroundHostsWhenOccupancyEmpty).toHaveBeenCalledWith('conv1')
  })

  it('handleBackgroundJobs forwards nested terminal occupancy jobs', () => {
    const conv = sampleConversation()
    const setBackgroundJobCount = vi.fn()
    const ctx = createMockStreamHandlerContext([conv], { setBackgroundJobCount })
    const jobs = [
      {
        jobId: 'job_term',
        status: 'running',
        kind: 'terminal',
        title: 'python scrape.py'
      }
    ]
    handleBackgroundJobs(ctx, {
      kind: 'background_jobs',
      conversationId: 'conv1',
      runningCount: 1,
      jobs
    })
    expect(setBackgroundJobCount).toHaveBeenCalledWith('conv1', 1, jobs)
  })
})
