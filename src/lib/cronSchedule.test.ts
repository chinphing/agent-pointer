import { describe, expect, it } from 'vitest'
import {
  buildCron,
  describeCron,
  parseCron,
  type CronPreset
} from './cronSchedule'

describe('buildCron', () => {
  it('everyMinute', () => {
    expect(buildCron({ mode: 'everyMinute' })).toBe('0 * * * * *')
  })

  it('everyNMinutes', () => {
    expect(buildCron({ mode: 'everyNMinutes', interval: 15 })).toBe('0 */15 * * * *')
  })

  it('everyNHours', () => {
    expect(buildCron({ mode: 'everyNHours', interval: 6 })).toBe('0 0 */6 * * *')
  })

  it('dailyAt', () => {
    expect(buildCron({ mode: 'dailyAt', hour: 9, minute: 30 })).toBe('0 30 9 * * *')
  })

  it('weeklyAt (Monday)', () => {
    expect(buildCron({ mode: 'weeklyAt', weekday: 1, hour: 9, minute: 30 })).toBe('0 30 9 * * 1')
  })

  it('weeklyAt (Sunday=0)', () => {
    expect(buildCron({ mode: 'weeklyAt', weekday: 0, hour: 0, minute: 0 })).toBe('0 0 0 * * 0')
  })

  it('monthlyAt (1st, 09:30)', () => {
    expect(buildCron({ mode: 'monthlyAt', dayOfMonth: 1, hour: 9, minute: 30 })).toBe('0 30 9 1 * *')
  })

  it('monthlyAt clamps day-of-month to 1..31', () => {
    expect(buildCron({ mode: 'monthlyAt', dayOfMonth: 0, hour: 9, minute: 0 })).toBe('0 0 9 1 * *')
    expect(buildCron({ mode: 'monthlyAt', dayOfMonth: 32, hour: 9, minute: 0 })).toBe('0 0 9 31 * *')
  })

  it('custom preserves raw', () => {
    expect(buildCron({ mode: 'custom', raw: '0 0 0 1 1 *' })).toBe('0 0 0 1 1 *')
  })

  it('clamps out-of-range params', () => {
    expect(buildCron({ mode: 'dailyAt', hour: 25, minute: -3 } as CronPreset)).toBe('0 0 23 * * *')
  })
})

describe('parseCron round-trips presets', () => {
  const cases: CronPreset[] = [
    { mode: 'everyMinute' },
    { mode: 'everyNMinutes', interval: 15 },
    { mode: 'everyNHours', interval: 6 },
    { mode: 'dailyAt', hour: 9, minute: 30 },
    { mode: 'weeklyAt', weekday: 1, hour: 9, minute: 30 },
    { mode: 'weeklyAt', weekday: 0, hour: 0, minute: 0 },
    { mode: 'monthlyAt', dayOfMonth: 1, hour: 9, minute: 30 },
    { mode: 'monthlyAt', dayOfMonth: 15, hour: 0, minute: 0 }
  ]
  for (const p of cases) {
    it(`${p.mode}: ${JSON.stringify(p)}`, () => {
      expect(parseCron(buildCron(p))).toEqual(p)
    })
  }

  it('falls back to custom for non-preset expressions', () => {
    expect(parseCron('0 0 0 1 1 *')).toEqual({ mode: 'custom', raw: '0 0 0 1 1 *' })
  })

  it('falls back to custom for wrong field count', () => {
    expect(parseCron('0 * * * *')).toEqual({ mode: 'custom', raw: '0 * * * *' })
  })

  it('empty string yields default preset', () => {
    expect(parseCron('').mode).toBe('dailyAt')
  })
})

describe('describeCron', () => {
  it('everyMinute', () => {
    expect(describeCron('0 * * * * *')).toBe('每分钟执行')
  })

  it('everyNMinutes', () => {
    expect(describeCron('0 */15 * * * *')).toBe('每 15 分钟执行')
  })

  it('everyNHours', () => {
    expect(describeCron('0 0 */6 * * *')).toBe('每 6 小时执行')
  })

  it('dailyAt zero-pads', () => {
    expect(describeCron('0 5 9 * * *')).toBe('每天 09:05 执行')
  })

  it('weeklyAt', () => {
    expect(describeCron('0 30 9 * * 1')).toBe('每周一 09:30 执行')
    expect(describeCron('0 0 0 * * 0')).toBe('每周日 00:00 执行')
  })

  it('monthlyAt', () => {
    expect(describeCron('0 30 9 1 * *')).toBe('每月 1 日 09:30 执行')
    expect(describeCron('0 0 0 15 * *')).toBe('每月 15 日 00:00 执行')
  })

  it('custom', () => {
    expect(describeCron('0 0 0 1 1 *')).toBe('自定义：0 0 0 1 1 *')
  })
})
