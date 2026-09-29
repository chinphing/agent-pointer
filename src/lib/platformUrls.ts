import { openExternalUrl } from './openExternalUrl'

function viteEnv(name: string): string {
  if (typeof import.meta === 'undefined') return ''
  const env = (import.meta as ImportMeta & { env?: Record<string, string> }).env
  return String(env?.[name] ?? '').trim()
}

/** Packaging flavour: `official`, or empty for a personal build. Gates nothing. */
export function pointerEdition(): string {
  return viteEnv('VITE_POINTER_EDITION').toLowerCase()
}

export function isOfficialEdition(): boolean {
  return pointerEdition() === 'official'
}

/**
 * Web base for platform pages (billing, …). Supplied by the build
 * (`VITE_POINTER_WEB_BASE`); the open-source tree hardcodes no vendor domain, so
 * a build without it returns empty and callers stay inert.
 */
export function platformWebBase(): string {
  return viteEnv('VITE_POINTER_WEB_BASE').replace(/\/$/, '')
}

/** Official account billing / WeChat recharge page. Empty for personal builds. */
export function platformBillingUrl(): string {
  const base = platformWebBase()
  if (!base) return ''
  return `${base}/profile/billing`
}

export async function openPlatformBillingPage(): Promise<void> {
  const url = platformBillingUrl()
  if (!url) {
    console.warn('platformUrls: billing page skipped (no web base in this edition)')
    return
  }
  await openExternalUrl(url)
}

export function isBalanceExhaustedMessage(text: string | null | undefined): boolean {
  const t = (text ?? '').trim()
  if (!t) return false
  return (
    t.includes('token_quota_exhausted') ||
    t.includes('账户余额已用尽') ||
    t.includes('账户余额不足')
  )
}
