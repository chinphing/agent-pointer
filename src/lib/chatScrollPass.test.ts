import { describe, expect, it, vi } from 'vitest'
import { createScrollPassScheduler, createViewedStampDedupe } from './chatScrollPass'

/** Manual frame clock: nothing runs until `flush()`. */
function createFrameHost() {
  const callbacks = new Map<number, () => void>()
  let nextHandle = 1
  return {
    request: vi.fn((callback: () => void) => {
      const handle = nextHandle
      nextHandle += 1
      callbacks.set(handle, callback)
      return handle
    }),
    cancel: vi.fn((handle: number) => {
      callbacks.delete(handle)
    }),
    flush() {
      const pending = [...callbacks.values()]
      callbacks.clear()
      for (const callback of pending) callback()
    },
    pendingCount() {
      return callbacks.size
    }
  }
}

describe('scroll pass scheduling', () => {
  it('coalesces every scroll event inside one frame into a single pass', () => {
    const host = createFrameHost()
    const run = vi.fn()
    const pass = createScrollPassScheduler(run, host)

    for (let i = 0; i < 5; i += 1) pass.schedule()

    expect(host.request).toHaveBeenCalledTimes(1)
    expect(pass.pending).toBe(true)
    expect(run).not.toHaveBeenCalled()

    host.flush()

    expect(run).toHaveBeenCalledTimes(1)
    expect(pass.pending).toBe(false)

    // The next frame schedules again instead of staying latched.
    pass.schedule()
    expect(host.request).toHaveBeenCalledTimes(2)
    host.flush()
    expect(run).toHaveBeenCalledTimes(2)
  })

  it('runs the pass against the scroll position of the newest event in the frame', () => {
    const host = createFrameHost()
    const scroller = { scrollTop: 0 }
    const seen: number[] = []
    const pass = createScrollPassScheduler(() => seen.push(scroller.scrollTop), host)

    scroller.scrollTop = 10
    pass.schedule()
    scroller.scrollTop = 40
    pass.schedule()
    scroller.scrollTop = 90
    pass.schedule()
    host.flush()

    expect(seen).toEqual([90])
  })

  it('drops a scheduled pass when it is cancelled before the frame', () => {
    const host = createFrameHost()
    const run = vi.fn()
    const pass = createScrollPassScheduler(run, host)

    pass.schedule()
    pass.cancel()

    expect(host.cancel).toHaveBeenCalledTimes(1)
    expect(pass.pending).toBe(false)
    expect(host.pendingCount()).toBe(0)

    host.flush()
    expect(run).not.toHaveBeenCalled()

    // A cancelled pass must not leave the scheduler latched.
    pass.schedule()
    host.flush()
    expect(run).toHaveBeenCalledTimes(1)
  })
})

describe('viewed user-message stamps', () => {
  it('stamps only the ids that entered the viewport since the previous pass', () => {
    const dedupe = createViewedStampDedupe()
    const stamp = vi.fn()

    dedupe.apply('c1', ['u1', 'u2'], stamp)
    expect(stamp.mock.calls).toEqual([['u1'], ['u2']])

    // Same visible set: the previous pass already stamped both rows.
    stamp.mockClear()
    dedupe.apply('c1', ['u1', 'u2'], stamp)
    expect(stamp).not.toHaveBeenCalled()

    // One new row scrolled in — only that one is written.
    stamp.mockClear()
    dedupe.apply('c1', ['u2', 'u3'], stamp)
    expect(stamp.mock.calls).toEqual([['u3']])
  })

  it('re-stamps an id that left and re-entered the viewport', () => {
    const dedupe = createViewedStampDedupe()
    const stamp = vi.fn()

    dedupe.apply('c1', ['u1'], stamp)
    dedupe.apply('c1', ['u2'], stamp)
    stamp.mockClear()

    dedupe.apply('c1', ['u1'], stamp)

    expect(stamp.mock.calls).toEqual([['u1']])
  })

  it('forgets the remembered set when the conversation changes', () => {
    const dedupe = createViewedStampDedupe()
    const stamp = vi.fn()

    dedupe.apply('c1', ['u1'], stamp)
    stamp.mockClear()

    dedupe.apply('c2', ['u1'], stamp)

    expect(stamp.mock.calls).toEqual([['u1']])
  })

  it('re-stamps every visible id after a reset (parked viewport renewal)', () => {
    const dedupe = createViewedStampDedupe()
    const stamp = vi.fn()

    dedupe.apply('c1', ['u1', 'u2'], stamp)
    stamp.mockClear()

    dedupe.reset()
    dedupe.apply('c1', ['u1', 'u2'], stamp)

    expect(stamp.mock.calls).toEqual([['u1'], ['u2']])
  })
})
