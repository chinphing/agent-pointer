/** Transient network/server failures — safe to auto-retry uploads / send. */

export type RetryOptions = {
  /** Total tries including the first (default 3). */
  maxAttempts?: number
  /** Delay before attempt 2; doubles each time (default 800ms). */
  baseDelayMs?: number
  shouldRetry?: (err: unknown, attempt: number) => boolean
  onRetry?: (err: unknown, nextAttempt: number, delayMs: number) => void
}

function defaultShouldRetry(err: unknown): boolean {
  const msg = (err instanceof Error ? err.message : String(err)).toLowerCase()
  if (!msg.trim()) return true
  // Permanent / client-side conditions — do not burn retries.
  if (
    /缺少上传|缺少附件|请先登录|platform_login|账户余额|不支持|无法创建会话|413|payload too large|entity too large|文件过大|unsupported/.test(
      msg
    )
  ) {
    return false
  }
  if (/\b401\b|\b403\b|\b404\b|\b422\b/.test(msg)) return false
  return true
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => {
    const schedule = globalThis.setTimeout.bind(globalThis) as typeof setTimeout
    schedule(resolve, ms)
  })
}

export async function withRetries<T>(
  fn: (attempt: number) => Promise<T>,
  options: RetryOptions = {}
): Promise<T> {
  const maxAttempts = Math.max(1, options.maxAttempts ?? 3)
  const baseDelayMs = Math.max(0, options.baseDelayMs ?? 800)
  const shouldRetry = options.shouldRetry ?? ((err, _attempt) => defaultShouldRetry(err))

  let lastErr: unknown
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    try {
      return await fn(attempt)
    } catch (err) {
      lastErr = err
      const willRetry = attempt < maxAttempts && shouldRetry(err, attempt)
      if (!willRetry) throw err
      const delayMs = baseDelayMs * 2 ** (attempt - 1)
      console.warn('[retry] attempt failed, will retry', {
        attempt,
        maxAttempts,
        delayMs,
        err
      })
      options.onRetry?.(err, attempt + 1, delayMs)
      if (delayMs > 0) await sleep(delayMs)
    }
  }
  throw lastErr instanceof Error ? lastErr : new Error(String(lastErr))
}
