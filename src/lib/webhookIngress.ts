/** Generic webhook ingress URL template (`POST /api/webhooks/:src`). */
export const WEBHOOK_URL_TEMPLATE = 'http://{host}:{port}/api/webhooks/{src}'

const WEBHOOK_TOKEN_CACHE_KEY = 'pointer.webhook.tokenSecrets.v1'

export function webhookConversationId(src: string): string {
  return `webhook:${src}`
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
