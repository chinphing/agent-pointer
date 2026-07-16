import { openExternalUrl } from './openExternalUrl'

/** Matches `platform_endpoints::DEFAULT_WEB_BASE` (release). Override with `VITE_POINTER_WEB_BASE`. */
const DEFAULT_WEB_BASE = 'https://pointer.readflowai.com'

export function platformWebBase(): string {
  const fromEnv =
    typeof import.meta !== 'undefined'
      ? String((import.meta as ImportMeta & { env?: Record<string, string> }).env?.VITE_POINTER_WEB_BASE ?? '').trim()
      : ''
  const base = (fromEnv || DEFAULT_WEB_BASE).replace(/\/$/, '')
  return base
}

/** Official account billing / WeChat recharge page. */
export function platformBillingUrl(): string {
  return `${platformWebBase()}/profile/billing`
}

export async function openPlatformBillingPage(): Promise<void> {
  await openExternalUrl(platformBillingUrl())
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
