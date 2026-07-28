// @vitest-environment happy-dom

import { beforeEach, describe, expect, it } from 'vitest'
import {
  elapsedBetweenTimestamps,
  formatTurnElapsed,
  hasActiveTurn,
  recordTurnDone,
  recordTurnStart,
  resolveTurnElapsedMs,
  turnElapsedMs
} from './turnElapsed'

describe('turn elapsed', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('records dispatch-to-done elapsed time by conversation and user turn', () => {
    recordTurnStart('conv-1', 'user-1', 1_000)
    expect(hasActiveTurn('conv-1')).toBe(true)

    expect(recordTurnDone('conv-1', 66_400)).toBe(65_400)
    expect(hasActiveTurn('conv-1')).toBe(false)
    expect(turnElapsedMs('conv-1', 'user-1')).toBe(65_400)
    expect(turnElapsedMs('conv-1', 'other-user')).toBeNull()
  })

  it('survives a storage read and keeps conversations isolated', () => {
    recordTurnStart('conv-1', 'user-1', 5_000)
    recordTurnStart('conv-2', 'user-2', 10_000)
    recordTurnDone('conv-1', 7_500)

    expect(turnElapsedMs('conv-1', 'user-1')).toBe(2_500)
    expect(turnElapsedMs('conv-2', 'user-2')).toBeNull()
    expect(recordTurnDone('conv-2', 14_000)).toBe(4_000)
  })

  it('finalizes the interrupted turn when force-send starts the next turn', () => {
    recordTurnStart('conv-1', 'user-a', 1_000)
    recordTurnStart('conv-1', 'user-b', 4_000)

    expect(turnElapsedMs('conv-1', 'user-a')).toBe(3_000)
    expect(hasActiveTurn('conv-1')).toBe(true)
    expect(recordTurnDone('conv-1', 9_000)).toBe(5_000)
    expect(turnElapsedMs('conv-1', 'user-b')).toBe(5_000)
  })

  it('derives elapsed time from persisted message timestamps', () => {
    expect(elapsedBetweenTimestamps(10_000, 75_400)).toBe(65_400)
    expect(elapsedBetweenTimestamps(75_400, 10_000)).toBeNull()
    expect(elapsedBetweenTimestamps(Number.NaN, 10_000)).toBeNull()
  })

  it('prefers recorded dispatch→Done timing over message createdAt span', () => {
    recordTurnStart('conv-1', 'user-1', 1_000)
    recordTurnDone('conv-1', 61_000)

    // createdAt span includes queue wait (enqueue at 0, last msg at 70s) — must not win.
    expect(
      resolveTurnElapsedMs({
        conversationId: 'conv-1',
        turnId: 'user-1',
        userCreatedAt: 0,
        lastMessageCreatedAt: 70_000
      })
    ).toBe(60_000)
  })

  it('falls back to createdAt span when no recorded timing exists', () => {
    expect(
      resolveTurnElapsedMs({
        conversationId: 'conv-1',
        turnId: 'user-missing',
        userCreatedAt: 10_000,
        lastMessageCreatedAt: 25_000
      })
    ).toBe(15_000)
  })

  it('formats Cursor-style minutes and zero-padded seconds with an honest fallback', () => {
    expect(formatTurnElapsed(125_999)).toBe('工作 2 m 05 s')
    expect(formatTurnElapsed(900)).toBe('工作 0 m 00 s')
    expect(formatTurnElapsed(null)).toBe('工作耗时未知')
  })
})
