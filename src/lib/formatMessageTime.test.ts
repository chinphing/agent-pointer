import { describe, expect, it } from 'vitest'
import { formatMessageTimeClock, formatMessageTimeFull } from './formatMessageTime'

describe('formatMessageTime', () => {
  it('shows hours, minutes, and seconds on the footer clock', () => {
    const ts = new Date(2026, 7, 16, 9, 5, 7).getTime()
    expect(formatMessageTimeClock(ts)).toBe('09:05:07')
  })

  it('keeps the full date in the tooltip', () => {
    const ts = new Date(2026, 7, 16, 21, 18, 3).getTime()
    expect(formatMessageTimeFull(ts)).toBe('2026-08-16 21:18:03')
  })
})
