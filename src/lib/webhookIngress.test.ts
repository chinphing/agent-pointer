import { describe, expect, it } from 'vitest'
import {
  WEBHOOK_URL_TEMPLATE,
  generateWebhookToken,
  webhookConversationId,
  currentWebhookSessionId,
  resolveWebhookViewSessionId,
  webhookIngressCurl,
  webhookIngressUrl
} from './webhookIngress'

describe('webhookIngress', () => {
  it('uses host port placeholders', () => {
    expect(WEBHOOK_URL_TEMPLATE).toBe('http://{host}:{port}/api/webhooks/{src}')
    expect(webhookIngressUrl('github')).toBe('http://{host}:{port}/api/webhooks/github')
    expect(webhookConversationId('github')).toBe('webhook:github')
  })

  it('derives dated session id from reset boundary', () => {
    const now = new Date(2026, 5, 28, 10, 0, 0)
    expect(currentWebhookSessionId('github', now)).toBe('webhook:github:20260628')
  })

  it('resolveWebhookViewSessionId prefers persisted id', () => {
    expect(
      resolveWebhookViewSessionId({
        src: 'github',
        currentSessionId: 'webhook:github:20260628',
        conversationId: 'webhook:github',
        hasTranscript: true
      })
    ).toBe('webhook:github:20260628')
    expect(
      resolveWebhookViewSessionId({
        src: 'github',
        conversationId: 'webhook:github',
        hasTranscript: true
      })
    ).toBe('webhook:github')
  })

  it('generates 32-char token', () => {
    expect(generateWebhookToken()).toMatch(/^[0-9a-f]{32}$/)
  })

  it('builds curl with real token and default blocking params', () => {
    const curl = webhookIngressCurl('github', 'secret-token-123')
    expect(curl).toContain('http://{host}:{port}/api/webhooks/github')
    expect(curl).toContain('Authorization: Bearer secret-token-123')
    expect(curl).toContain('"blocking":true')
    expect(curl).toContain('"timeoutSeconds":120')
  })
})
