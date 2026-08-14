import { describe, expect, it } from 'vitest'
import {
  isLowPlatformBalance,
  LOW_PLATFORM_BALANCE_YUAN,
  shouldShowPlatformBalance
} from './usePlatformBalance'

describe('platform balance display rules', () => {
  it('shows only for a logged-in desktop platform account', () => {
    expect(shouldShowPlatformBalance(true, false, true)).toBe(true)
    expect(shouldShowPlatformBalance(false, false, true)).toBe(false)
    expect(shouldShowPlatformBalance(true, true, true)).toBe(false)
    expect(shouldShowPlatformBalance(true, false, false)).toBe(false)
  })

  it('marks finite non-negative balances below the shared threshold as low', () => {
    expect(isLowPlatformBalance(String(LOW_PLATFORM_BALANCE_YUAN - 0.01))).toBe(true)
    expect(isLowPlatformBalance(String(LOW_PLATFORM_BALANCE_YUAN))).toBe(false)
    expect(isLowPlatformBalance('0')).toBe(true)
    expect(isLowPlatformBalance('-1')).toBe(false)
    expect(isLowPlatformBalance('unknown')).toBe(false)
    expect(isLowPlatformBalance(null)).toBe(false)
  })
})
