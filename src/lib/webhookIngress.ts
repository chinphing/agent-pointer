/** Generic webhook ingress URL template (`POST /api/webhooks/:src`). */
export const WEBHOOK_URL_TEMPLATE = 'http://{host}:{port}/api/webhooks/{src}'

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
  sessionMode?: 'daily' | 'per_delivery'
}): string | null {
  const persisted = source.currentSessionId?.trim()
  const legacy = webhookConversationId(source.src)
  if (persisted && persisted !== legacy) return persisted
  const mode = source.sessionMode ?? 'per_delivery'
  if (mode === 'per_delivery') return null
  return currentWebhookSessionId(source.src)
}

export function webhookIngressUrl(src: string): string {
  return `http://{host}:{port}/api/webhooks/${src}`
}

/** 32-char hex token (UUID without hyphens). */
export function generateWebhookToken(): string {
  return crypto.randomUUID().replace(/-/g, '')
}

export function webhookIngressCurl(
  src: string,
  token: string,
  authHeaderName?: string | null
): string {
  const url = webhookIngressUrl(src)
  const body = JSON.stringify({
    text: 'hello',
    blocking: true,
    timeoutSeconds: 120
  })
  const customHeader = authHeaderName?.trim()
  const authLine = customHeader
    ? `  -H '${customHeader}: ${token}' \\`
    : `  -H 'Authorization: Bearer ${token}' \\`
  return [
    `curl -X POST '${url}' \\`,
    authLine,
    `  -H 'Content-Type: application/json' \\`,
    `  -d '${body}'`
  ].join('\n')
}
