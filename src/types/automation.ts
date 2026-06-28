/**
 * Automation (cron + webhook) types shared by the desktop (Tauri IPC) and web
 * (HTTP) runtimes. Field names are camelCase to match the backend views in
 * `pointer-core` (`CronJobView` / `WebhookConfigView`).
 */

export interface CronJob {
  id: string
  label: string
  cronExpr: string
  conversationId: string
  /** Active cron session id (`cron:{jobId}:{yyyymmdd}`); null until first fire. */
  currentSessionId?: string | null
  promptText: string
  agentMode?: string | null
  leadAgentId?: string | null
  enabled: boolean
  lastRunAtMs?: number | null
  nextRunAtMs?: number | null
  createdAtMs: number
}

export interface CreateCronJobInput {
  id: string
  label: string
  cronExpr: string
  /** Ignored by the backend: each cron job owns a dedicated `cron:{id}` session. */
  conversationId?: string
  promptText: string
  agentMode?: string | null
  leadAgentId?: string | null
  enabled?: boolean
}

export interface UpdateCronJobInput {
  enabled?: boolean
}

/** One configured webhook ingress source with masked token preview. */
export interface WebhookSource {
  /** URL path segment (`POST /api/webhooks/{src}`). */
  src: string
  preview: string
  /** Full ingress URL (web/server only; empty on desktop). */
  url?: string
  /** Stable source key (`webhook:{src}`). */
  conversationId: string
  /** Active session id (`webhook:{src}:{yyyymmdd}`); null until first ingress. */
  currentSessionId?: string | null
  /** Whether any session for this source has stored messages. */
  hasTranscript: boolean
  /** Custom auth header; omit for default Bearer + X-Pointer-Token. */
  authHeaderName?: string | null
}

/**
 * Webhook config: per-source tokens. Each source has its own Bearer token;
 * ingress rejects requests when the token does not match the `:src` path.
 * `legacyConfigured` indicates the deprecated single global token (applies to
 * all sources until cleared and replaced with per-source tokens).
 */
export interface WebhookConfig {
  sources: WebhookSource[]
  urlTemplate: string
  legacyConfigured: boolean
  legacyPreview?: string | null
}

export interface SetWebhookSourceInput {
  src: string
  token: string
  authHeaderName?: string | null
}
