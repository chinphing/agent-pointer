/**
 * Automation (cron + webhook) types shared by the desktop (Tauri IPC) and web
 * (HTTP) runtimes. Field names are camelCase to match the backend views in
 * `pointer-core` (`CronJobView` / `WebhookConfigView`).
 */

export interface CronJob {
  id: string
  label: string
  cronExpr: string
  /** `cron` (recurring) or `once` (one-shot soft-complete). */
  scheduleKind?: string
  /** Original schedule string (e.g. `30m`, `daily@9:30`). */
  scheduleRaw?: string | null
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
  /**
   * Optional Run → IM delivery spec (e.g. "feishu", "feishu:ou_xxx",
   * "feishu:group:oc_xxx", comma-separated, "all"). Empty / null = no IM push
   * after the run; the cron session still records the transcript.
   */
  deliver?: string | null
  /** Last IM delivery error; cleared when a later delivery succeeds. */
  lastDeliveryError?: string | null
}

export interface CreateCronJobInput {
  id: string
  label: string
  /** Recurring 6-field cron; optional when `schedule` is set. */
  cronExpr?: string
  /** Friendly / one-shot schedule (`30m`, ISO, `daily@9:30`). Preferred. */
  schedule?: string
  /** Ignored by the backend: each cron job owns a dedicated `cron:{id}` session. */
  conversationId?: string
  promptText: string
  agentMode?: string | null
  leadAgentId?: string | null
  enabled?: boolean
  /** Optional Run → IM delivery spec. See `CronJob.deliver`. */
  deliver?: string | null
}

export interface UpdateCronJobInput {
  enabled?: boolean
  /** Optional Run → IM deliver spec. Pass empty string to clear. */
  deliver?: string | null
}

/** One channel deliver option from `GET /api/cron-jobs/delivery-targets`. */
export interface CronDeliveryTarget {
  deliver: string
  channel: string
  accountId: string
  label: string
  recipientId: string
  isGroup: boolean
  isHome: boolean
  /** False when the channel is enabled but has no binding yet. */
  bound: boolean
  displayName?: string | null
}

export type WebhookSessionMode = 'daily' | 'per_delivery'

/** One configured webhook ingress source (token omitted on web HTTP API). */
export interface WebhookSource {
  /** URL path segment (`POST /api/webhooks/{src}`). */
  src: string
  /** Full bearer token (desktop IPC only; empty on web after refresh). */
  token?: string
  /** Masked preview (`****` + last 4). */
  preview: string
  /** Full ingress URL (web/server only; empty on desktop). */
  url?: string
  /** Stable source key (`webhook:{src}`). */
  conversationId: string
  /** Active session id; daily = dated, per_delivery = latest delivery. */
  currentSessionId?: string | null
  /** Whether any session for this source has stored messages. */
  hasTranscript: boolean
  /** Custom auth header; omit for default Bearer + X-Pointer-Token. */
  authHeaderName?: string | null
  /** `per_delivery` (default) or `daily`. */
  sessionMode?: WebhookSessionMode
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
  sessionMode?: WebhookSessionMode
}

/** Poll response for `GET /api/webhooks/:src/runs/:runId`. */
export interface WebhookRunView {
  runId: string
  status: string
  conversationId: string
  text?: string
  error?: string
}

/** Full bearer token returned by settings reveal API. */
export interface WebhookTokenReveal {
  src: string
  token: string
  preview: string
}

/** Dispatcher per-lane queue waiter (settings UI). */
export interface QueueWaiterView {
  runId: string
  conversationId: string
  triggerSource: string
}

/** Per-lane queue depth in the nested lane registry. */
export interface LaneQueueView {
  lane: string
  active: number
  waiting: number
  maxConcurrent: number
  waiters: QueueWaiterView[]
}

/** Persisted run still waiting for a lane slot (`runs.status = queued`). */
export interface PendingRunView {
  runId: string
  conversationId: string
  triggerSource: string
  createdAtMs: number
}

/** Combined dispatcher queue snapshot for settings / observability. */
export interface BackgroundJobOccupancyView {
  conversationId: string
  runningCount: number
}

export interface RunQueueSnapshot {
  maxConcurrentMain: number
  maxConcurrentCron: number
  lanes: LaneQueueView[]
  pendingRuns: PendingRunView[]
  /** Jobs still queued/running. Independent of dispatcher lanes. */
  backgroundJobs?: BackgroundJobOccupancyView[]
}
