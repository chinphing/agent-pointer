import { describe, expect, it } from 'vitest'
import {
  normalizeUiLocalePreference,
  resolveUiLocale
} from './uiLocale'

describe('normalizeUiLocalePreference', () => {
  it('accepts known values', () => {
    expect(normalizeUiLocalePreference('system')).toBe('system')
    expect(normalizeUiLocalePreference('zh-CN')).toBe('zh-CN')
    expect(normalizeUiLocalePreference('en')).toBe('en')
  })

  it('falls back to system for unknown input', () => {
    expect(normalizeUiLocalePreference('')).toBe('system')
    expect(normalizeUiLocalePreference('fr')).toBe('system')
    expect(normalizeUiLocalePreference(undefined)).toBe('system')
  })
})

describe('resolveUiLocale', () => {
  it('honors explicit preference', () => {
    expect(resolveUiLocale('zh-CN', 'en-US')).toBe('zh-CN')
    expect(resolveUiLocale('en', 'zh-CN')).toBe('en')
  })

  it('maps system + zh* navigator to zh-CN', () => {
    expect(resolveUiLocale('system', 'zh-CN')).toBe('zh-CN')
    expect(resolveUiLocale('system', 'zh-TW')).toBe('zh-CN')
    expect(resolveUiLocale('system', 'zh')).toBe('zh-CN')
  })

  it('maps system + non-zh navigator to en', () => {
    expect(resolveUiLocale('system', 'en-US')).toBe('en')
    expect(resolveUiLocale('system', 'ja-JP')).toBe('en')
    expect(resolveUiLocale('system', '')).toBe('en')
  })
})
