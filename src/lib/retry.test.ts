import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { withRetries } from './retry'

describe('withRetries', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('returns on first success', async () => {
    const fn = vi.fn().mockResolvedValue('ok')
    await expect(withRetries(fn, { maxAttempts: 3, baseDelayMs: 10 })).resolves.toBe('ok')
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('retries transient failures then succeeds', async () => {
    const fn = vi
      .fn()
      .mockRejectedValueOnce(new Error('网络错误，上传失败'))
      .mockResolvedValueOnce('ok')
    const p = withRetries(fn, { maxAttempts: 3, baseDelayMs: 100 })
    await vi.advanceTimersByTimeAsync(100)
    await expect(p).resolves.toBe('ok')
    expect(fn).toHaveBeenCalledTimes(2)
  })

  it('does not retry permanent errors', async () => {
    const fn = vi.fn().mockRejectedValue(new Error('请先登录 Pointer 账户'))
    await expect(withRetries(fn, { maxAttempts: 3, baseDelayMs: 10 })).rejects.toThrow(/请先登录/)
    expect(fn).toHaveBeenCalledTimes(1)
  })
})
