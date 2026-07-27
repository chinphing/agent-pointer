// @vitest-environment happy-dom

import { beforeEach, describe, expect, it } from 'vitest'
import {
  elapsedBetweenTimestamps,
  formatTurnElapsed,
  hasActiveTurn,
  recordTurnDone,
  recordTurnStart,
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

  it('derives elapsed time from persisted message timestamps', () => {
    expect(elapsedBetweenTimestamps(10_000, 75_400)).toBe(65_400)
    expect(elapsedBetweenTimestamps(75_400, 10_000)).toBeNull()
    expect(elapsedBetweenTimestamps(Number.NaN, 10_000)).toBeNull()
  })

  it('formats Cursor-style minutes and zero-padded seconds with an honest fallback', () => {
    expect(formatTurnElapsed(125_999)).toBe('工作 2 m 05 s')
    expect(formatTurnElapsed(900)).toBe('工作 0 m 00 s')
    expect(formatTurnElapsed(null)).toBe('工作耗时未知')
  })
})
