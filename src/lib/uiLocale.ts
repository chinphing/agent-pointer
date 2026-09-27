/** UI language preference persisted in user_settings.json. */
export type UiLocalePreference = 'system' | 'zh-CN' | 'en'

/** Resolved BCP-47 locale used by vue-i18n and Rust catalogs. */
export type ResolvedUiLocale = 'zh-CN' | 'en'

const STORAGE_KEY = 'pointer-ui-locale'

let currentPref: UiLocalePreference = 'system'

export function normalizeUiLocalePreference(
  raw: string | null | undefined
): UiLocalePreference {
  const v = (raw ?? '').trim()
  if (v === 'zh-CN' || v === 'en' || v === 'system') return v
  return 'system'
}

/**
 * Resolve the active UI locale from preference + host/OS language.
 * `system`: zh* → zh-CN, otherwise en.
 */
export function resolveUiLocale(
  pref: UiLocalePreference | string | null | undefined,
  navigatorLanguage?: string | null
): ResolvedUiLocale {
  const p = normalizeUiLocalePreference(
    typeof pref === 'string' ? pref : pref ?? undefined
  )
  if (p === 'zh-CN' || p === 'en') return p
  const lang =
    (navigatorLanguage ??
      (typeof navigator !== 'undefined' ? navigator.language : '') ??
      '')
      .trim()
      .toLowerCase()
  if (lang.startsWith('zh')) return 'zh-CN'
  return 'en'
}

export function readStoredUiLocalePreference(): UiLocalePreference {
  if (typeof localStorage === 'undefined') return 'system'
  try {
    return normalizeUiLocalePreference(localStorage.getItem(STORAGE_KEY))
  } catch (e) {
    console.warn('[i18n] failed to read stored uiLocale', e)
    return 'system'
  }
}

function persistUiLocalePreference(pref: UiLocalePreference) {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(STORAGE_KEY, pref)
  } catch (e) {
    console.warn('[i18n] failed to persist uiLocale', e)
  }
}

export function getUiLocalePreference(): UiLocalePreference {
  return currentPref
}

type LocaleApplier = (locale: ResolvedUiLocale) => void

let applyLocaleImpl: LocaleApplier | null = null

/** Called once from i18n bootstrap so settings can switch locale without circular imports. */
export function registerUiLocaleApplier(fn: LocaleApplier) {
  applyLocaleImpl = fn
}

/**
 * Apply UI locale preference immediately (vue-i18n + localStorage cache).
 * Does not persist to user_settings.json — callers save via settings store.
 */
export function applyUiLocale(pref: UiLocalePreference | string | null | undefined) {
  currentPref = normalizeUiLocalePreference(
    typeof pref === 'string' ? pref : pref ?? undefined
  )
  persistUiLocalePreference(currentPref)
  const resolved = resolveUiLocale(currentPref)
  if (applyLocaleImpl) {
    applyLocaleImpl(resolved)
  } else {
    console.warn('[i18n] applyUiLocale called before i18n applier registered')
  }
  if (typeof document !== 'undefined') {
    document.documentElement.lang = resolved === 'zh-CN' ? 'zh-CN' : 'en'
  }
  console.info('[i18n] uiLocale=%s resolved=%s', currentPref, resolved)
}

/** Bootstrap from localStorage before settings load (same pattern as theme). */
export function applyUiLocaleBootstrap() {
  applyUiLocale(readStoredUiLocalePreference())
}
