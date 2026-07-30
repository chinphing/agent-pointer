import { describe, expect, it, vi, afterEach } from 'vitest'
import { randomUuid } from './randomUuid'

const UUID_V4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

describe('randomUuid', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('uses crypto.randomUUID when available', () => {
    const fixed = '11111111-2222-4333-8444-555555555555'
    vi.stubGlobal('crypto', {
      randomUUID: () => fixed,
      getRandomValues: (arr: Uint8Array) => arr
    })
    expect(randomUuid()).toBe(fixed)
  })

  it('falls back to getRandomValues when randomUUID is missing', () => {
    vi.stubGlobal('crypto', {
      getRandomValues: (arr: Uint8Array) => {
        for (let i = 0; i < arr.length; i++) arr[i] = i
        return arr
      }
    })
    const id = randomUuid()
    expect(id).toMatch(UUID_V4)
  })

  it('falls back to Math.random when crypto is unavailable', () => {
    vi.stubGlobal('crypto', undefined)
    const id = randomUuid()
    expect(id).toMatch(UUID_V4)
  })
})
