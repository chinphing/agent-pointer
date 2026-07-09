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
    const fallback = resolveWebhookViewSessionId({
      src: 'github',
      conversationId: 'webhook:github',
      hasTranscript: true
    })
    expect(fallback).toBeNull()
    expect(
      resolveWebhookViewSessionId({
        src: 'github',
        conversationId: 'webhook:github',
        hasTranscript: true,
        sessionMode: 'daily'
      })
    ).toMatch(/^webhook:github:\d{8}$/)
    expect(
      resolveWebhookViewSessionId({
        src: 'github',
        currentSessionId: 'webhook:github',
        sessionMode: 'daily'
      })
    ).toMatch(/^webhook:github:\d{8}$/)
  })

  it('per_delivery without session returns null', () => {
    expect(
      resolveWebhookViewSessionId({
        src: 'github',
        sessionMode: 'per_delivery',
        conversationId: 'webhook:github'
      })
    ).toBeNull()
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

  it('builds curl with custom auth header', () => {
    const curl = webhookIngressCurl('codeup', 'tok', 'X-Codeup-Token')
    expect(curl).toContain("X-Codeup-Token: tok")
    expect(curl).not.toContain('Authorization: Bearer')
  })
})
