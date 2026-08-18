import { describe, expect, it } from 'vitest'
import {
  CHARS_PER_THINKING_DOT,
  MAX_THINKING_DOT_CHARS,
  MAX_THINKING_DOTS,
  THINKING_DOTS_PER_TIER,
  THINKING_LIVE_DOT_COUNT,
  bodyHasVisibleUserFacingOutput,
  charsCoveredByThinkingDots,
  shouldShowThinkingIndicator,
  thinkingDotCount,
  thinkingDotsAtCap,
  thinkingLabel,
  thinkingSteadyDotCount
} from './thinkingIndicator'

describe('thinking dots (tiered)', () => {
  it('caps at 27800 chars for 48 dots', () => {
    expect(THINKING_DOTS_PER_TIER).toBe(10)
    expect(CHARS_PER_THINKING_DOT).toBe(100)
    expect(MAX_THINKING_DOT_CHARS).toBe(27800)
    expect(charsCoveredByThinkingDots(MAX_THINKING_DOTS)).toBe(27800)
  })

  it('starts at 1 dot and uses 100 chars per dot in the first tier', () => {
    expect(thinkingDotCount(0)).toBe(1)
    expect(thinkingDotCount(1)).toBe(1)
    expect(thinkingDotCount(100)).toBe(1)
    expect(thinkingDotCount(101)).toBe(2)
    expect(thinkingDotCount(1000)).toBe(10)
  })

  it('doubles chars-per-dot after every 10 dots', () => {
    expect(thinkingDotCount(1001)).toBe(11)
    expect(thinkingDotCount(1000 + 200)).toBe(11)
    expect(thinkingDotCount(1000 + 201)).toBe(12)
    expect(thinkingDotCount(3000)).toBe(20)
    expect(thinkingDotCount(3001)).toBe(21)
    expect(thinkingDotCount(7000)).toBe(30)
    expect(thinkingDotCount(15000)).toBe(40)
    expect(thinkingDotCount(15001)).toBe(41)
    expect(thinkingDotCount(27800)).toBe(48)
    expect(thinkingDotCount(27801)).toBe(48)
  })

  it('thinkingLabel repeats the computed dots', () => {
    expect(thinkingLabel(0)).toBe('思考中.')
    expect(thinkingLabel(1001)).toBe(`思考中${'.'.repeat(11)}`)
  })

  it('keeps 45 steady dots at the cap so 3 can stay live', () => {
    expect(thinkingDotsAtCap(26200)).toBe(false)
    expect(thinkingDotsAtCap(26201)).toBe(true)
    expect(thinkingSteadyDotCount(101)).toBe(2)
    expect(thinkingSteadyDotCount(27800)).toBe(45)
    expect(thinkingSteadyDotCount(27800) + THINKING_LIVE_DOT_COUNT).toBe(MAX_THINKING_DOTS)
  })
})

describe('shouldShowThinkingIndicator', () => {
  it('hides once user-facing content or tools exist, even if thoughts are still long', () => {
    expect(bodyHasVisibleUserFacingOutput({ thoughts: 'x'.repeat(30_000) })).toBe(false)
    expect(
      bodyHasVisibleUserFacingOutput({
        thoughts: 'x'.repeat(30_000),
        content: '继续 m2。'
      })
    ).toBe(true)
    expect(
      bodyHasVisibleUserFacingOutput({
        thoughts: 'x'.repeat(30_000),
        toolCalls: [{ id: 't1', name: 'grep', arguments: '{}', status: 'success' }]
      })
    ).toBe(true)

    expect(
      shouldShowThinkingIndicator({
        runInProgress: true,
        body: { thoughts: 'x'.repeat(30_000) }
      })
    ).toBe(true)
    expect(
      shouldShowThinkingIndicator({
        runInProgress: true,
        body: { thoughts: 'x'.repeat(30_000), content: '继续 m2。' }
      })
    ).toBe(false)
    expect(
      shouldShowThinkingIndicator({
        runInProgress: true,
        extraVisibleTools: true,
        body: { thoughts: 'x'.repeat(30_000) }
      })
    ).toBe(false)
  })
})
