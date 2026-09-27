import type { Composer } from 'vue-i18n'

/** Persisted UI language preference. */
export type UiLocalePreference = 'system' | 'zh-CN' | 'en'

/** Resolved locale passed to vue-i18n / Rust catalogs. */
export type ResolvedUiLocale = 'zh-CN' | 'en'

export const UI_LOCALE_STORAGE_KEY = 'pointer-ui-locale'

export const UI_LOCALE_OPTIONS: { value: UiLocalePreference; labelKey: string }[] = [
  { value: 'system', labelKey: 'settings.language.system' },
  { value: 'zh-CN', labelKey: 'settings.language.zhCN' },
  { value: 'en', labelKey: 'settings.language.en' }
]

let currentPref: UiLocalePreference = 'system'
// Loosely typed so createI18n's typed Composer still binds cleanly.
let i18nRef: { locale: { value: string } } | null = null

export function isUiLocalePreference(value: unknown): value is UiLocalePreference {
  return value === 'system' || value === 'zh-CN' || value === 'en'
}

/** Map OS / browser language to a supported UI locale. */
export function detectSystemUiLocale(): ResolvedUiLocale {
  if (typeof navigator === 'undefined') return 'en'
  const candidates = [
    ...(navigator.languages ?? []),
    navigator.language
  ].filter(Boolean)
  for (const raw of candidates) {
    const lower = String(raw).toLowerCase()
    if (lower.startsWith('zh')) return 'zh-CN'
  }
  return 'en'
}

export function resolveUiLocale(pref: UiLocalePreference | undefined | null): ResolvedUiLocale {
  const p = isUiLocalePreference(pref) ? pref : 'system'
  if (p === 'zh-CN' || p === 'en') return p
  return detectSystemUiLocale()
}

export function bindI18nComposer(composer: { locale: { value: string } }) {
  i18nRef = composer
}

/** Apply preference to vue-i18n and cache for bootstrap. Instant — no restart. */
export function applyUiLocale(pref: UiLocalePreference | undefined | null) {
  currentPref = isUiLocalePreference(pref) ? pref : 'system'
  const resolved = resolveUiLocale(currentPref)
  try {
    localStorage.setItem(UI_LOCALE_STORAGE_KEY, currentPref)
  } catch {
    /* ignore quota / private mode */
  }
  if (i18nRef) {
    i18nRef.locale.value = resolved
  }
  if (typeof document !== 'undefined') {
    document.documentElement.lang = resolved === 'zh-CN' ? 'zh-CN' : 'en'
  }
  console.info('[i18n] uiLocale=%s resolved=%s', currentPref, resolved)
  return resolved
}

/** Read cached preference before settings API loads. */
export function applyUiLocaleBootstrap() {
  try {
    const cached = localStorage.getItem(UI_LOCALE_STORAGE_KEY)
    if (isUiLocalePreference(cached)) {
      applyUiLocale(cached)
      return
    }
  } catch {
    /* ignore */
  }
  applyUiLocale('system')
}

export function currentUiLocalePreference(): UiLocalePreference {
  return currentPref
}

export function currentResolvedUiLocale(): ResolvedUiLocale {
  return resolveUiLocale(currentPref)
}
