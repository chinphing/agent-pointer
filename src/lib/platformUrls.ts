import { openExternalUrl } from './openExternalUrl'

/** Matches `platform_endpoints` official default. Personal builds leave this empty. */
const OFFICIAL_WEB_BASE = 'https://pointer.readflowai.com'

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

export function platformWebBase(): string {
  const fromEnv = viteEnv('VITE_POINTER_WEB_BASE')
  const fallback = isOfficialEdition() ? OFFICIAL_WEB_BASE : ''
  return (fromEnv || fallback).replace(/\/$/, '')
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
