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

/**
 * Webhook token config. `preview` is a masked token (e.g. `****1234`) shown
 * after the one-time assignment; the raw token is never returned to the UI.
 * `urlTemplate` is empty on desktop (no HTTP ingress); the web/server build
 * fills in `{base}/api/webhooks/{src}`.
 */
export interface WebhookConfig {
  configured: boolean
  preview?: string | null
  urlTemplate: string
}
