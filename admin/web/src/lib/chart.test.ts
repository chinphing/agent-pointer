import { describe, expect, it } from 'vitest'
import { fmtCompact, makeTicks, niceMax } from './chart'

describe('fmtCompact', () => {
  it('keeps small numbers intact', () => {
    expect(fmtCompact(0)).toBe('0')
    expect(fmtCompact(7)).toBe('7')
    expect(fmtCompact(999)).toBe('999')
  })

  it('compresses thousands / millions / billions', () => {
    expect(fmtCompact(1000)).toBe('1K')
    expect(fmtCompact(1234)).toBe('1.2K')
    expect(fmtCompact(200000)).toBe('200K')
    expect(fmtCompact(2602500)).toBe('2.6M')
    expect(fmtCompact(1_000_000_000)).toBe('1B')
  })

  it('drops the redundant .0', () => {
    expect(fmtCompact(2000)).toBe('2K')
    expect(fmtCompact(3_000_000)).toBe('3M')
  })

  it('keeps the sign', () => {
    expect(fmtCompact(-1234)).toBe('-1.2K')
  })

  it('treats junk as zero instead of NaN', () => {
    expect(fmtCompact(null)).toBe('0')
    expect(fmtCompact(undefined)).toBe('0')
    expect(fmtCompact(Number.NaN)).toBe('0')
  })
})

describe('niceMax', () => {
  it('rounds up to a readable ceiling', () => {
    expect(niceMax(912500)).toBe(1_000_000)
    expect(niceMax(200000)).toBe(200000)
    expect(niceMax(1)).toBe(1)
  })

  it('never returns zero for empty input', () => {
    expect(niceMax(0)).toBe(1)
    expect(niceMax(-5)).toBe(1)
  })

  it('is never below the data', () => {
    for (const v of [1, 7, 33, 480, 910, 1200, 33333, 912500, 4_400_000]) {
      expect(niceMax(v)).toBeGreaterThanOrEqual(v)
    }
  })
})

describe('makeTicks', () => {
  it('produces divisions + 1 ticks from 0 to the top', () => {
    const ticks = makeTicks(912500, 4)
    expect(ticks).toHaveLength(5)
    expect(ticks[0].value).toBe(0)
    expect(ticks[0].pct).toBe(0)
    expect(ticks[4].value).toBe(1_000_000)
    expect(ticks[4].pct).toBe(100)
  })

  it('gives evenly spaced percentages', () => {
    const ticks = makeTicks(200000, 4)
    expect(ticks.map((t) => t.pct)).toEqual([0, 25, 50, 75, 100])
    expect(ticks.map((t) => t.label)).toEqual(['0', '50K', '100K', '150K', '200K'])
  })

  it('does not divide by zero on empty data', () => {
    const ticks = makeTicks(0, 4)
    expect(ticks[4].value).toBe(1)
    expect(ticks.every((t) => Number.isFinite(t.pct))).toBe(true)
  })
})
