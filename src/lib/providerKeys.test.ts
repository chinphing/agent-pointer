import { describe, expect, it } from 'vitest'
import { hasAnyProviderKey } from './providerKeys'

describe('hasAnyProviderKey', () => {
  it('accepts the merged default provider key', () => {
    expect(hasAnyProviderKey({ hasKey: true, providers: [] })).toBe(true)
  })

  it('accepts a key that only a non-default provider has', () => {
    // Regression: gating the composer on the merged hasKey blocked turns whose
    // agent + tier resolution would have picked a provider that does have a key.
    expect(
      hasAnyProviderKey({ hasKey: false, providers: [{ apiKey: '' }, { apiKey: ' sk-x ' }] })
    ).toBe(true)
  })

  it('rejects settings without any key', () => {
    expect(hasAnyProviderKey({ hasKey: false, providers: [{ apiKey: '' }, {}] })).toBe(false)
    expect(hasAnyProviderKey({ hasKey: false })).toBe(false)
    expect(hasAnyProviderKey({})).toBe(false)
  })
})
