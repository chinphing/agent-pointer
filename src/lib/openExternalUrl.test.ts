// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { openExternalUrl } from './openExternalUrl'

/**
 * 「点了没反应」的那一层。
 *
 * `platformBillingUrl()` 在控制面未配置时返回空串，这个空串最终落到这里。
 * 这里必须**什么都不做**：不要 `window.open('')`（有的浏览器会打开当前页），
 * 也不要把非 http(s) 的东西丢给系统。
 */
const opened: string[] = []

afterEach(() => {
  opened.length = 0
  vi.restoreAllMocks()
})

function stubWindowOpen() {
  vi.spyOn(window, 'open').mockImplementation(((url: string) => {
    opened.push(String(url))
    return null
  }) as typeof window.open)
}

describe('openExternalUrl', () => {
  it('ignores empty input', async () => {
    stubWindowOpen()
    await openExternalUrl('')
    await openExternalUrl('   ')
    expect(opened).toEqual([])
  })

  it('rejects non-http protocols', async () => {
    stubWindowOpen()
    await openExternalUrl('javascript:alert(1)')
    await openExternalUrl('file:///etc/passwd')
    expect(opened).toEqual([])
  })

  it('opens normal https links', async () => {
    stubWindowOpen()
    await openExternalUrl('https://pointer.example.com/me/billing')
    expect(opened).toEqual(['https://pointer.example.com/me/billing'])
  })
})
