// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { applyTheme, applyThemeBootstrap, resolveThemeMode } from './theme'

type SchemeMql = MediaQueryList & { dispatch: (dark: boolean) => void }

function installMatchMedia(dark: boolean): SchemeMql {
  const listeners = new Set<(ev: MediaQueryListEvent) => void>()
  const mql = {
    matches: dark,
    media: '(prefers-color-scheme: dark)',
    onchange: null,
    addEventListener(_type: string, cb: EventListenerOrEventListenerObject) {
      if (typeof cb === 'function') listeners.add(cb as (ev: MediaQueryListEvent) => void)
    },
    removeEventListener(_type: string, cb: EventListenerOrEventListenerObject) {
      if (typeof cb === 'function') listeners.delete(cb as (ev: MediaQueryListEvent) => void)
    },
    addListener() {},
    removeListener() {},
    dispatchEvent() {
      return true
    },
    dispatch(nextDark: boolean) {
      mql.matches = nextDark
      const ev = { matches: nextDark } as MediaQueryListEvent
      listeners.forEach(cb => cb(ev))
    }
  }
  vi.stubGlobal('matchMedia', () => mql)
  return mql as SchemeMql
}

describe('theme', () => {
  beforeEach(() => {
    document.documentElement.className = ''
    document.documentElement.style.colorScheme = ''
    localStorage.clear()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('resolves system from prefers-color-scheme', () => {
    installMatchMedia(true)
    expect(resolveThemeMode('system')).toBe('dark')
    installMatchMedia(false)
    expect(resolveThemeMode('system')).toBe('light')
    expect(resolveThemeMode('dark')).toBe('dark')
    expect(resolveThemeMode('light')).toBe('light')
  })

  it('re-applies when system appearance changes and preference is system', () => {
    const mql = installMatchMedia(false)
    applyTheme('system')
    expect(document.documentElement.classList.contains('light')).toBe(true)
    expect(document.documentElement.style.colorScheme).toBe('light dark')

    mql.dispatch(true)
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(document.documentElement.classList.contains('light')).toBe(false)
    expect(document.documentElement.style.colorScheme).toBe('light dark')
  })

  it('does not follow system appearance when preference is locked', () => {
    const mql = installMatchMedia(false)
    applyTheme('light')
    expect(document.documentElement.style.colorScheme).toBe('light')
    mql.dispatch(true)
    expect(document.documentElement.classList.contains('light')).toBe(true)
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })

  it('bootstraps from cached system preference', () => {
    installMatchMedia(true)
    localStorage.setItem('pointer-theme', 'system')
    applyThemeBootstrap()
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })
})
