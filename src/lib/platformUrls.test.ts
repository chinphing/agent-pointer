import { afterEach, describe, expect, it } from 'vitest'
import {
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
})
