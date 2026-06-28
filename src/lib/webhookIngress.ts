/** Generic webhook ingress URL template (`POST /api/webhooks/:src`). */
export const WEBHOOK_URL_TEMPLATE = 'http://{host}:{port}/api/webhooks/{src}'

const WEBHOOK_TOKEN_CACHE_KEY = 'pointer.webhook.tokenSecrets.v1'

const WEBHOOK_SESSION_RESET_AT_HOUR = 4

export function webhookConversationId(src: string): string {
  return `webhook:${src}`
}

/** Active webhook session id at `now` (04:00 local rollover). */
export function currentWebhookSessionId(src: string, now = new Date()): string {
  const boundary = dailyResetAt(now, WEBHOOK_SESSION_RESET_AT_HOUR)
  const y = boundary.getFullYear()
  const m = String(boundary.getMonth() + 1).padStart(2, '0')
  const d = String(boundary.getDate()).padStart(2, '0')
  return `webhook:${src}:${y}${m}${d}`
}

function dailyResetAt(now: Date, atHour: number): Date {
  const reset = new Date(now)
  reset.setHours(atHour, 0, 0, 0)
  if (now.getTime() < reset.getTime()) {
    reset.setDate(reset.getDate() - 1)
  }
  return reset
}

/** Resolve the session id the UI should open for a webhook source row. */
export function resolveWebhookViewSessionId(source: {
  src: string
  currentSessionId?: string | null
  conversationId?: string
  hasTranscript?: boolean
}): string | null {
  const persisted = source.currentSessionId?.trim()
  if (persisted) return persisted
  if (source.hasTranscript && source.conversationId?.trim()) {
    return source.conversationId.trim()
  }
  return null
}

export function webhookIngressUrl(src: string): string {
  return `http://{host}:{port}/api/webhooks/${src}`
}

/** 32-char hex token (UUID without hyphens). */
export function generateWebhookToken(): string {
  return crypto.randomUUID().replace(/-/g, '')
}

export function webhookIngressCurl(src: string, token: string): string {
  const url = webhookIngressUrl(src)
  const body = JSON.stringify({
    text: 'hello',
    blocking: true,
    timeoutSeconds: 120
  })
  return [
    `curl -X POST '${url}' \\`,
    `  -H 'Authorization: Bearer ${token}' \\`,
    `  -H 'Content-Type: application/json' \\`,
    `  -d '${body}'`
  ].join('\n')
}

export function loadWebhookTokenCache(): Record<string, string> {
  try {
    const raw = sessionStorage.getItem(WEBHOOK_TOKEN_CACHE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as Record<string, string>
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

export function rememberWebhookToken(src: string, token: string): void {
  const trimmed = token.trim()
  if (!trimmed) return
  const cache = loadWebhookTokenCache()
  cache[src] = trimmed
  sessionStorage.setItem(WEBHOOK_TOKEN_CACHE_KEY, JSON.stringify(cache))
}

export function recallWebhookToken(src: string): string | undefined {
  return loadWebhookTokenCache()[src]
}

export function forgetWebhookToken(src: string): void {
  const cache = loadWebhookTokenCache()
  if (!(src in cache)) return
  delete cache[src]
  sessionStorage.setItem(WEBHOOK_TOKEN_CACHE_KEY, JSON.stringify(cache))
}
