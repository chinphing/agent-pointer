import type { ThemePreference } from '../types/chat'

const STORAGE_KEY = 'pointer-theme'

export function resolveThemeMode(pref: ThemePreference | undefined): 'light' | 'dark' {
  const p = pref ?? 'system'
  if (p === 'light' || p === 'dark') return p
  if (typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches) {
    return 'dark'
  }
  return 'light'
}

export function applyTheme(pref: ThemePreference | undefined) {
  const mode = resolveThemeMode(pref)
  const root = document.documentElement
  root.classList.remove('light', 'dark')
  root.classList.add(mode)
  try {
    localStorage.setItem(STORAGE_KEY, pref ?? 'system')
  } catch {
    /* ignore */
  }
}

/** Apply cached preference before settings API loads. */
export function applyThemeBootstrap() {
  try {
    const cached = localStorage.getItem(STORAGE_KEY) as ThemePreference | null
    if (cached === 'light' || cached === 'dark' || cached === 'system') {
      applyTheme(cached)
      return
    }
  } catch {
    /* ignore */
  }
  applyTheme('dark')
}
