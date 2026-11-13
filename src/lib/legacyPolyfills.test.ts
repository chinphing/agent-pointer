// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import { installLegacyPolyfills } from './legacyPolyfills'

describe('installLegacyPolyfills', () => {
  it('不覆盖浏览器已有的实现', () => {
    const existing = function NativeResizeObserver() {}
    const before = window.ResizeObserver
    ;(window as unknown as { ResizeObserver: unknown }).ResizeObserver = existing
    installLegacyPolyfills()
    expect(window.ResizeObserver).toBe(existing)
    ;(window as unknown as { ResizeObserver: unknown }).ResizeObserver = before
  })

  it('缺失时补上 ResizeObserver', () => {
    const before = window.ResizeObserver
    // @ts-expect-error 故意制造缺失
    delete window.ResizeObserver
    installLegacyPolyfills()
    expect(typeof window.ResizeObserver).toBe('function')
    ;(window as unknown as { ResizeObserver: unknown }).ResizeObserver = before
  })

  it('缺失时补上 crypto.randomUUID，且输出合法 v4', () => {
    const original = crypto.randomUUID
    // @ts-expect-error 故意制造缺失
    delete crypto.randomUUID
    installLegacyPolyfills()
    const id = crypto.randomUUID()
    expect(id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
    )
    expect(crypto.randomUUID()).not.toBe(id)
    ;(crypto as unknown as { randomUUID: () => string }).randomUUID = original
  })

  it('缺失时补上 queueMicrotask，回调在同步代码之后执行', async () => {
    const original = window.queueMicrotask
    // @ts-expect-error 故意制造缺失
    delete window.queueMicrotask
    installLegacyPolyfills()
    const order: string[] = []
    window.queueMicrotask(() => order.push('micro'))
    order.push('sync')
    await Promise.resolve()
    expect(order).toEqual(['sync', 'micro'])
    window.queueMicrotask = original
  })
})
