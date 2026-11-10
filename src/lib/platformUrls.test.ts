import { afterEach, describe, expect, it } from 'vitest'
import {
  initPlatformWebBase,
  platformBillingUrl,
  platformDownloadUrl,
  platformWebBase,
  setPlatformWebBase,
} from './platformUrls'

afterEach(() => setPlatformWebBase(''))

describe('platformUrls', () => {
  it('trims the trailing slash of the bound base', () => {
    setPlatformWebBase('  https://pointer.example.com/  ')
    expect(platformWebBase()).toBe('https://pointer.example.com')
  })

  it('builds billing and download from the runtime base', () => {
    setPlatformWebBase('https://pointer.example.com')
    // 控制面与官网用同一组路径，客户端不按部署分支
    expect(platformBillingUrl()).toBe('https://pointer.example.com/me/billing')
    expect(platformDownloadUrl()).toBe('https://pointer.example.com/download')
  })

  it('never builds a download URL from a host other than the base', () => {
    // 曾经有过第二个来源（VITE_POINTER_DOWNLOAD_URL），未绑定时会跳去另一个域名。
    setPlatformWebBase('https://pointer.example.com')
    const base = platformWebBase()
    expect(platformDownloadUrl().startsWith(`${base}/`)).toBe(true)
  })

  it('is a no-op without a Tauri IPC bridge', async () => {
    // 纯 Web 构建没有 window.__TAURI_INTERNALS__：必须安静返回，
    // 而不是抛 TypeError 让调用方去吞。
    setPlatformWebBase('')
    await expect(initPlatformWebBase()).resolves.toBeUndefined()
  })
})
