import { createI18n } from 'vue-i18n'
import en from '../locales/en.json'
import zhCN from '../locales/zh-CN.json'
import {
  applyUiLocaleBootstrap,
  registerUiLocaleApplier,
  resolveUiLocale,
  readStoredUiLocalePreference,
  type ResolvedUiLocale
} from '../lib/uiLocale'

const initialLocale = resolveUiLocale(readStoredUiLocalePreference())

export const i18n = createI18n({
  legacy: false,
  globalInjection: true,
  locale: initialLocale,
  fallbackLocale: 'en',
  messages: {
    'zh-CN': zhCN,
    en
  },
  missingWarn: import.meta.env.DEV,
  fallbackWarn: import.meta.env.DEV
})

registerUiLocaleApplier((locale: ResolvedUiLocale) => {
  i18n.global.locale.value = locale
})

/** Apply cached preference so first paint matches last session before settings load. */
applyUiLocaleBootstrap()

/** Translate outside Vue setup (stores, lib helpers). */
export function t(key: string, named?: Record<string, unknown>): string {
  // vue-i18n Compose API typings accept MessageParams; keep call site simple.
  return named
    ? (i18n.global.t as (k: string, v: Record<string, unknown>) => string)(key, named)
    : (i18n.global.t as (k: string) => string)(key)
}

export function te(key: string): boolean {
  return i18n.global.te(key)
}
