import { createI18n } from 'vue-i18n'
import en from './locales/en.json'
import zhCN from './locales/zh-CN.json'

/**
 * 管理端国际化，做法与客户端一致：vue-i18n + 两份完整语言包 + 键集合校验。
 *
 * 语言存在 localStorage（管理端没有「设置」页，不做账号级同步）。
 * 缺键时回退中文：管理端的主要读者是中文用户，回退英文会让人看不懂。
 */
const STORAGE_KEY = 'pointer-admin-locale'

export type AdminLocale = 'zh-CN' | 'en'

export function readStoredLocale(): AdminLocale {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw === 'en' || raw === 'zh-CN') return raw
  } catch {
    /* 隐私模式下 localStorage 会抛错，忽略即可 */
  }
  return 'zh-CN'
}

export const i18n = createI18n({
  legacy: false,
  globalInjection: true,
  locale: readStoredLocale(),
  fallbackLocale: 'zh-CN',
  messages: { 'zh-CN': zhCN, en },
})

export function setLocale(locale: AdminLocale) {
  i18n.global.locale.value = locale
  try {
    localStorage.setItem(STORAGE_KEY, locale)
  } catch {
    /* 存不下也不影响本次会话 */
  }
}

export function currentLocale(): AdminLocale {
  return i18n.global.locale.value as AdminLocale
}

/** 在 setup 之外（store / lib）取文案。 */
export function t(key: string, named?: Record<string, unknown>): string {
  return named
    ? (i18n.global.t as (k: string, v: Record<string, unknown>) => string)(key, named)
    : (i18n.global.t as (k: string) => string)(key)
}
