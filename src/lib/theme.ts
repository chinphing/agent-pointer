import type { ThemePreference } from '../types/chat'
import { isTauriRuntime } from './runtime'

const STORAGE_KEY = 'pointer-theme'
const SYSTEM_DARK_QUERY = '(prefers-color-scheme: dark)'

let currentPref: ThemePreference = 'system'
let subscribedMql: MediaQueryList | null = null
let lastNativePref: ThemePreference | null = null
let nativeListenerBound = false

export function resolveThemeMode(pref: ThemePreference | undefined): 'light' | 'dark' {
  const p = pref ?? 'system'
  if (p === 'light' || p === 'dark') return p
  if (typeof window !== 'undefined' && window.matchMedia(SYSTEM_DARK_QUERY).matches) {
    return 'dark'
  }
  return 'light'
}

function onSystemSchemeChange() {
  if (currentPref !== 'system') return
  applyTheme('system')
}

function detachMediaListener(mql: MediaQueryList | null) {
  if (!mql) return
  if (typeof mql.removeEventListener === 'function') {
    mql.removeEventListener('change', onSystemSchemeChange)
  } else {
    const legacy = mql as MediaQueryList & {
      removeListener?: (cb: () => void) => void
    }
    legacy.removeListener?.(onSystemSchemeChange)
  }
}

function ensureSystemThemeListener() {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return
  const mql = window.matchMedia(SYSTEM_DARK_QUERY)
  if (subscribedMql === mql) return
  detachMediaListener(subscribedMql)
  subscribedMql = mql
  if (typeof mql.addEventListener === 'function') {
    mql.addEventListener('change', onSystemSchemeChange)
  } else {
    const legacy = mql as MediaQueryList & {
      addListener?: (cb: () => void) => void
    }
    legacy.addListener?.(onSystemSchemeChange)
  }
}

async function syncNativeWindowTheme(pref: ThemePreference) {
  if (!isTauriRuntime()) return
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    const win = getCurrentWindow()
    if (lastNativePref !== pref) {
      lastNativePref = pref
      await win.setTheme(pref === 'system' ? null : pref)
    }
    if (nativeListenerBound) return
    nativeListenerBound = true
    await win.onThemeChanged(() => {
      if (currentPref !== 'system') return
      applyTheme('system')
    })
  } catch (e) {
    console.warn('[theme] native window theme sync failed', e)
  }
}

export function applyTheme(pref: ThemePreference | undefined) {
  currentPref = pref ?? 'system'
  const mode = resolveThemeMode(currentPref)
  const root = document.documentElement
  root.classList.remove('light', 'dark')
  root.classList.add(mode)
  // `light dark` keeps native widgets and `prefers-color-scheme` tied to the OS.
  // A locked value would pin the media query and block follow-system updates.
  root.style.colorScheme = currentPref === 'system' ? 'light dark' : mode
  try {
    localStorage.setItem(STORAGE_KEY, currentPref)
  } catch {
    /* ignore */
  }
  ensureSystemThemeListener()
  void syncNativeWindowTheme(currentPref)
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
