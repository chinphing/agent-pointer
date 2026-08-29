import { describe, expect, it } from 'vitest'
import {
  canShowNoOlderPullHint,
  LOAD_NEWER_BOTTOM_PX,
  LOAD_OLDER_LEAVE_TOP_PX,
  LOAD_OLDER_TOP_PX,
  shouldAutoPrefetchOlderOnScroll,
  shouldRearmOlderPrefetch,
  shouldRequestNewerFromTouchPull,
  shouldRequestNewerFromWheel,
  shouldRequestOlderFromTouchPull,
  shouldRequestOlderFromWheel
} from './messageListOlderPrefetch'

describe('shouldRequestOlderFromWheel', () => {
  it('loads older when the user wheels up at the real top', () => {
    expect(
      shouldRequestOlderFromWheel({
        deltaY: -40,
        scrollTop: 0,
        hasMoreOlder: true
      })
    ).toBe(true)
  })

  it('loads older when paging state is missing so self-heal can run', () => {
    expect(
      shouldRequestOlderFromWheel({
        deltaY: -12,
        scrollTop: 0,
        hasMoreOlder: null
      })
    ).toBe(true)
  })

  it('does not steal the no-older pull hint', () => {
    expect(
      shouldRequestOlderFromWheel({
        deltaY: -40,
        scrollTop: 0,
        hasMoreOlder: false
      })
    ).toBe(false)
  })

  it('ignores wheel-up far from the top', () => {
    expect(
      shouldRequestOlderFromWheel({
        deltaY: -40,
        scrollTop: LOAD_OLDER_TOP_PX + 1,
        hasMoreOlder: true
      })
    ).toBe(false)
  })

  it('ignores wheel-down', () => {
    expect(
      shouldRequestOlderFromWheel({
        deltaY: 40,
        scrollTop: 0,
        hasMoreOlder: true
      })
    ).toBe(false)
  })
})

describe('shouldRequestOlderFromTouchPull', () => {
  it('loads older when pulling down while already at the top', () => {
    expect(
      shouldRequestOlderFromTouchPull({
        pullPx: 20,
        scrollTop: 0,
        hasMoreOlder: true
      })
    ).toBe(true)
  })

  it('leaves native scroll alone when the list can still move', () => {
    expect(
      shouldRequestOlderFromTouchPull({
        pullPx: 40,
        scrollTop: 80,
        hasMoreOlder: true
      })
    ).toBe(false)
  })
})

describe('canShowNoOlderPullHint', () => {
  it('only shows after the pager says there is nothing older', () => {
    expect(
      canShowNoOlderPullHint({
        hasMoreOlder: false,
        scrollTop: 0,
        programmatic: false
      })
    ).toBe(true)
    expect(
      canShowNoOlderPullHint({
        hasMoreOlder: true,
        scrollTop: 0,
        programmatic: false
      })
    ).toBe(false)
    expect(
      canShowNoOlderPullHint({
        hasMoreOlder: null,
        scrollTop: 0,
        programmatic: false
      })
    ).toBe(false)
  })
})

describe('shouldAutoPrefetchOlderOnScroll', () => {
  it('requires a decreasing scrollTop, the top band, and being armed', () => {
    expect(
      shouldAutoPrefetchOlderOnScroll({
        atTop: true,
        scrollingUp: true,
        armed: true
      })
    ).toBe(true)
    expect(
      shouldAutoPrefetchOlderOnScroll({
        atTop: true,
        scrollingUp: false,
        armed: true
      })
    ).toBe(false)
    expect(
      shouldAutoPrefetchOlderOnScroll({
        atTop: true,
        scrollingUp: true,
        armed: false
      })
    ).toBe(false)
  })

  it('rearms only after leaving the top band', () => {
    expect(shouldRearmOlderPrefetch(LOAD_OLDER_LEAVE_TOP_PX)).toBe(false)
    expect(shouldRearmOlderPrefetch(LOAD_OLDER_LEAVE_TOP_PX + 1)).toBe(true)
  })
})

describe('shouldRequestNewerFromWheel', () => {
  it('loads newer when the user wheels down at the real bottom', () => {
    expect(
      shouldRequestNewerFromWheel({
        deltaY: 40,
        distanceFromBottom: 0,
        hasMoreNewer: true
      })
    ).toBe(true)
  })

  it('loads newer while still inside the bottom band', () => {
    expect(
      shouldRequestNewerFromWheel({
        deltaY: 12,
        distanceFromBottom: LOAD_NEWER_BOTTOM_PX,
        hasMoreNewer: true
      })
    ).toBe(true)
  })

  it('does nothing when the window is already the tail', () => {
    expect(
      shouldRequestNewerFromWheel({
        deltaY: 40,
        distanceFromBottom: 0,
        hasMoreNewer: false
      })
    ).toBe(false)
  })

  it('ignores wheel-down far from the bottom', () => {
    expect(
      shouldRequestNewerFromWheel({
        deltaY: 40,
        distanceFromBottom: LOAD_NEWER_BOTTOM_PX + 1,
        hasMoreNewer: true
      })
    ).toBe(false)
  })

  it('ignores wheel-up', () => {
    expect(
      shouldRequestNewerFromWheel({
        deltaY: -40,
        distanceFromBottom: 0,
        hasMoreNewer: true
      })
    ).toBe(false)
  })

  it('does not load newer when the list already ends on the generating turn', () => {
    expect(
      shouldRequestNewerFromWheel({
        deltaY: 40,
        distanceFromBottom: 0,
        hasMoreNewer: true,
        lastMessageIsLiveTail: true
      })
    ).toBe(false)
  })
})

describe('shouldRequestNewerFromTouchPull', () => {
  it('loads newer when flicking up while already at the bottom', () => {
    expect(
      shouldRequestNewerFromTouchPull({
        pullPx: 20,
        distanceFromBottom: 0,
        hasMoreNewer: true
      })
    ).toBe(true)
  })

  it('leaves native scroll alone when the list can still move', () => {
    expect(
      shouldRequestNewerFromTouchPull({
        pullPx: 40,
        distanceFromBottom: 80,
        hasMoreNewer: true
      })
    ).toBe(false)
  })
})
