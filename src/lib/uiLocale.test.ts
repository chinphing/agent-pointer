import { describe, expect, it } from 'vitest'
import {
  detectSystemUiLocale,
  isUiLocalePreference,
  resolveUiLocale
} from './uiLocale'

describe('uiLocale', () => {
  it('accepts known preferences', () => {
    expect(isUiLocalePreference('system')).toBe(true)
    expect(isUiLocalePreference('zh-CN')).toBe(true)
    expect(isUiLocalePreference('en')).toBe(true)
    expect(isUiLocalePreference('fr')).toBe(false)
  })

  it('resolves explicit locales', () => {
    expect(resolveUiLocale('zh-CN')).toBe('zh-CN')
    expect(resolveUiLocale('en')).toBe('en')
  })

  it('falls back to system for invalid input', () => {
    const resolved = resolveUiLocale(undefined)
    expect(resolved === 'zh-CN' || resolved === 'en').toBe(true)
    expect(resolved).toBe(detectSystemUiLocale())
  })
})
