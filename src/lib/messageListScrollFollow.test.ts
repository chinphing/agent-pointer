import { describe, expect, it } from 'vitest'
import {
  nextFollowOutputAfterScroll,
  scrollerViewportShrinkDelta,
  shouldSkipTotalSizeStickAfterViewportShrink,
  shouldSkipTotalSizeStick,
  isComposerDraftingTarget,
  switchConversationScrollPlan,
  toBottomFollowsOutput
} from './messageListScrollFollow'

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

describe('scrollerViewportShrinkDelta', () => {
  it('returns the lost viewport height when the scroller shrinks', () => {
    expect(scrollerViewportShrinkDelta(800, 760)).toBe(40)
  })

  it('does not compensate when the scroller grows or stays the same', () => {
    expect(scrollerViewportShrinkDelta(760, 800)).toBe(0)
    expect(scrollerViewportShrinkDelta(800, 800)).toBe(0)
  })

  it('ignores an uninitialized previous height so mount does not jump', () => {
    expect(scrollerViewportShrinkDelta(0, 800)).toBe(0)
  })
})

describe('shouldSkipTotalSizeStickAfterViewportShrink', () => {
  it('skips stick only inside the suppression window', () => {
    expect(shouldSkipTotalSizeStickAfterViewportShrink(1_000, 950, 100)).toBe(true)
    expect(shouldSkipTotalSizeStickAfterViewportShrink(1_000, 900, 100)).toBe(false)
    expect(shouldSkipTotalSizeStickAfterViewportShrink(1_000, 1_000, 100)).toBe(true)
    expect(shouldSkipTotalSizeStickAfterViewportShrink(1_000, 0, 100)).toBe(false)
  })
})

describe('shouldSkipTotalSizeStick', () => {
  it('keeps skipping after the time window while the composer is still tall or focused', () => {
    expect(
      shouldSkipTotalSizeStick({
        nowMs: 2_000,
        viewportShrinkAtMs: 1_000,
        windowMs: 120,
        skipUntilViewportGrows: true,
        composerDrafting: false
      })
    ).toBe(true)
    expect(
      shouldSkipTotalSizeStick({
        nowMs: 2_000,
        viewportShrinkAtMs: 1_000,
        windowMs: 120,
        skipUntilViewportGrows: false,
        composerDrafting: true
      })
    ).toBe(true)
    expect(
      shouldSkipTotalSizeStick({
        nowMs: 2_000,
        viewportShrinkAtMs: 1_000,
        windowMs: 120,
        skipUntilViewportGrows: false,
        composerDrafting: false
      })
    ).toBe(false)
  })
})

describe('isComposerDraftingTarget', () => {
  it('matches nodes inside the composer shell', () => {
    const inside = {
      closest: (sel: string) => (sel === '.composer-shell' ? {} : null)
    }
    const outside = {
      closest: () => null
    }
    expect(isComposerDraftingTarget(inside as unknown as EventTarget)).toBe(true)
    expect(isComposerDraftingTarget(outside as unknown as EventTarget)).toBe(false)
    expect(isComposerDraftingTarget(null)).toBe(false)
  })
})

describe('switchConversationScrollPlan', () => {
  it('lets search locate keep the around window', () => {
    expect(
      switchConversationScrollPlan({ hasPendingFocus: true, hasMoreNewer: true })
    ).toBe('locate')
  })

  it('jumps to the real tail when re-opening a hole window', () => {
    expect(
      switchConversationScrollPlan({ hasPendingFocus: false, hasMoreNewer: true })
    ).toBe('jumpToLatest')
  })

  it('sticks to bottom when the loaded window is already the tail', () => {
    expect(
      switchConversationScrollPlan({ hasPendingFocus: false, hasMoreNewer: false })
    ).toBe('toBottom')
  })
})

describe('toBottomFollowsOutput', () => {
  it('does not claim live follow on an around-window bottom', () => {
    expect(toBottomFollowsOutput(true)).toBe(false)
    expect(toBottomFollowsOutput(false)).toBe(true)
  })
})
