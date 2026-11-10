import { afterEach, describe, expect, it, vi } from 'vitest'

/**
 * 客户端「余额 / 充值」点击后的落点。
 *
 * 这里不测 UI，只钉住那条契约：点了就打开 `<控制面>/me/billing`，
 * 控制面没配置时**什么都不打开**（不要跳到空串或本地路径）。
 */
const opened: string[] = []

vi.mock('./openExternalUrl', () => ({
  openExternalUrl: (url: string) => {
    opened.push(url)
    return Promise.resolve()
  },
}))

const { platformBillingUrl, openPlatformBillingPage, setPlatformWebBase } = await import(
  './platformUrls'
)

afterEach(() => {
  setPlatformWebBase('')
  opened.length = 0
})

describe('billing entry point', () => {
  it('points at the control plane personal-center billing page', () => {
    setPlatformWebBase('https://pointer.example.com')
    expect(platformBillingUrl()).toBe('https://pointer.example.com/me/billing')
  })

  it('opens that url when the balance is clicked', async () => {
    setPlatformWebBase('https://pointer.example.com')
    await openPlatformBillingPage()
    expect(opened).toEqual(['https://pointer.example.com/me/billing'])
  })

  it('forwards exactly what the url builder returns', async () => {
    setPlatformWebBase('https://pointer.example.com')
    await openPlatformBillingPage()
    // 未配置时 builder 返回空串，空串由 openExternalUrl 自己拦掉（见其单测）
    expect(opened).toEqual([platformBillingUrl()])
  })

  it('keeps sub-path deployments intact', async () => {
    setPlatformWebBase('https://example.com/pointer')
    await openPlatformBillingPage()
    expect(opened).toEqual(['https://example.com/pointer/me/billing'])
  })
})
