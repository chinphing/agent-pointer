import { createI18n } from 'vue-i18n'
import en from '../locales/en.json'
import zhCN from '../locales/zh-CN.json'
import {
  applyUiLocaleBootstrap,
  bindI18nComposer,
  resolveUiLocale,
  type ResolvedUiLocale
} from '../lib/uiLocale'

const bootstrapLocale: ResolvedUiLocale = (() => {
  try {
    const cached = localStorage.getItem('pointer-ui-locale')
    if (cached === 'zh-CN' || cached === 'en' || cached === 'system') {
      return resolveUiLocale(cached)
    }
  } catch {
    /* ignore */
  }
  return resolveUiLocale('system')
})()

export const i18n = createI18n({
  legacy: false,
  locale: bootstrapLocale,
  fallbackLocale: 'en',
  messages: {
    en,
    'zh-CN': zhCN
  },
  missingWarn: import.meta.env.DEV,
  fallbackWarn: import.meta.env.DEV
})

bindI18nComposer(i18n.global)
applyUiLocaleBootstrap()

export function t(key: string, values?: Record<string, unknown>): string {
  return i18n.global.t(key, values as never) as string
}

export default i18n
