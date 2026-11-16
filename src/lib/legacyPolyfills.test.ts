// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
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

  describe('CSSStyleSheet 构造器', () => {
    const g = window as unknown as { CSSStyleSheet?: unknown }
    let before: unknown

    beforeEach(() => {
      before = g.CSSStyleSheet
    })

    afterEach(() => {
      g.CSSStyleSheet = before
    })

    it('原生不可构造时补上垫片（Chrome 63 的 Illegal constructor）', () => {
      // 模拟 Chrome 63：接口存在，但 new 会抛 Illegal constructor。
      g.CSSStyleSheet = function Chrome63StyleSheet() {
        throw new TypeError('Illegal constructor')
      }
      installLegacyPolyfills()
      expect(() => new (g.CSSStyleSheet as new () => unknown)()).not.toThrow()
    })

    it('垫片按 mermaid 的用法收集规则', () => {
      g.CSSStyleSheet = function Chrome63StyleSheet() {
        throw new TypeError('Illegal constructor')
      }
      installLegacyPolyfills()
      const Sheet = g.CSSStyleSheet as new () => {
        cssRules: Array<{ cssText: string }>
        insertRule: (rule: string, index?: number) => number
        replaceSync: (text: string) => void
      }
      const sheet = new Sheet()
      sheet.insertRule(':root { --a: 1 }', sheet.cssRules.length)
      sheet.insertRule(':root { --b: 2 }', sheet.cssRules.length)
      sheet.replaceSync('.x { color: red }')
      // mermaid 的 cssStyleSheetToString: [...cssRules].map(r => r.cssText).join(' ')
      expect([...sheet.cssRules].map((r) => r.cssText).join(' ')).toBe('.x { color: red }')
    })

    it('原生可构造时不覆盖', () => {
      class NativeSheet {}
      g.CSSStyleSheet = NativeSheet
      installLegacyPolyfills()
      expect(g.CSSStyleSheet).toBe(NativeSheet)
    })
  })
})
