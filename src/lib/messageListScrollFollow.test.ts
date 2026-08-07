import { describe, expect, it } from 'vitest'
import { nextFollowOutputAfterScroll } from './messageListScrollFollow'

describe('nextFollowOutputAfterScroll', () => {
  const attachPx = 8
  const detachPx = 48

  it('detaches immediately when scrolling toward older content', () => {
    expect(
      nextFollowOutputAfterScroll({
        followOutput: true,
        distanceFromBottom: 20,
        scrollingUp: true,
        attachPx,
        detachPx
      })
    ).toBe(false)
  })

  it('reattaches only when essentially at the bottom', () => {
    expect(
      nextFollowOutputAfterScroll({
        followOutput: false,
        distanceFromBottom: 4,
        scrollingUp: false,
        attachPx,
        detachPx
      })
    ).toBe(true)
  })

  it('detaches by distance past the detach threshold', () => {
    expect(
      nextFollowOutputAfterScroll({
        followOutput: true,
        distanceFromBottom: 60,
        scrollingUp: false,
        attachPx,
        detachPx
      })
    ).toBe(false)
  })

  it('keeps follow in the hysteresis band when not scrolling up', () => {
    expect(
      nextFollowOutputAfterScroll({
        followOutput: true,
        distanceFromBottom: 24,
        scrollingUp: false,
        attachPx,
        detachPx
      })
    ).toBe(true)
    expect(
      nextFollowOutputAfterScroll({
        followOutput: false,
        distanceFromBottom: 24,
        scrollingUp: false,
        attachPx,
        detachPx
      })
    ).toBe(false)
  })
})
