import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  clearStreamDeltaBuffers,
  clearToolArgsDeltaBufferForTool,
  CONTENT_DELTA_BATCH_MS,
  enqueueAssistantJsonPartial,
  enqueueContentDelta,
  enqueueToolArgsDelta,
  flushAssistantJsonPartialBuffer,
  flushContentDeltaBuffer,
  setAssistantJsonPartialApplyHandler,
  setContentDeltaApplyHandler,
  setToolArgsDeltaApplyHandler
} from './reasoningDeltaBatch'

afterEach(() => {
  clearStreamDeltaBuffers()
  setContentDeltaApplyHandler(null)
  setToolArgsDeltaApplyHandler(null)
  setAssistantJsonPartialApplyHandler(null)
  vi.useRealTimers()
})

describe('content delta batching', () => {
  it('coalesces deltas in order and supports synchronous terminal flush', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)

    enqueueContentDelta('m1', 'hel')
    enqueueContentDelta('m1', 'lo')
    expect(apply).not.toHaveBeenCalled()

    flushContentDeltaBuffer('m1')
    expect(apply).toHaveBeenCalledTimes(1)
    expect(apply).toHaveBeenCalledWith('m1', undefined, undefined, 'hello')
    vi.runAllTimers()
    expect(apply).toHaveBeenCalledTimes(1)
  })

  it('keeps message buffers isolated', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)

    enqueueContentDelta('m1', 'one')
    enqueueContentDelta('m2', 'two')
    flushContentDeltaBuffer('m1')

    expect(apply).toHaveBeenCalledWith('m1', undefined, undefined, 'one')
    expect(apply).not.toHaveBeenCalledWith('m2', undefined, undefined, 'two')
    vi.advanceTimersByTime(CONTENT_DELTA_BATCH_MS)
    expect(apply).toHaveBeenCalledWith('m2', undefined, undefined, 'two')
  })

  it('flushes all pending keys in one shared timer tick', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setContentDeltaApplyHandler(apply)

    enqueueContentDelta('m1', 'a')
    enqueueContentDelta('m2', 'b')
    enqueueContentDelta('m3', 'c')
    expect(apply).not.toHaveBeenCalled()

    vi.advanceTimersByTime(CONTENT_DELTA_BATCH_MS)
    expect(apply).toHaveBeenCalledTimes(3)
  })
})

describe('assistant json partial batching', () => {
  it('merges latest-wins fields and flushes once', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setAssistantJsonPartialApplyHandler(apply)

    enqueueAssistantJsonPartial('m1', { thoughts: 'a', toolName: 'search' })
    enqueueAssistantJsonPartial('m1', { thoughts: 'ab', responseText: 'hi' })
    expect(apply).not.toHaveBeenCalled()

    flushAssistantJsonPartialBuffer('m1')
    expect(apply).toHaveBeenCalledTimes(1)
    expect(apply).toHaveBeenCalledWith('m1', undefined, undefined, {
      thoughts: 'ab',
      toolName: 'search',
      responseText: 'hi'
    })
  })
})

describe('tool args delta batching', () => {
  it('discards pending args when an authoritative snapshot arrives', () => {
    vi.useFakeTimers()
    const apply = vi.fn()
    setToolArgsDeltaApplyHandler(apply)

    enqueueToolArgsDelta('m1', 'tc1', '}')
    clearToolArgsDeltaBufferForTool('m1', 'tc1')
    vi.advanceTimersByTime(100)

    expect(apply).not.toHaveBeenCalled()
  })
})
