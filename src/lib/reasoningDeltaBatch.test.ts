import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  clearStreamDeltaBuffers,
  enqueueContentDelta,
  flushContentDeltaBuffer,
  setContentDeltaApplyHandler
} from './reasoningDeltaBatch'

afterEach(() => {
  clearStreamDeltaBuffers()
  setContentDeltaApplyHandler(null)
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
    vi.advanceTimersByTime(50)
    expect(apply).toHaveBeenCalledWith('m2', undefined, undefined, 'two')
  })
})
