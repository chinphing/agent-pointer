import { t } from '../i18n'
import type { BackgroundJobView, ChatMessage, ToolCall } from '../types/chat'
import { isJobAwaitCall } from './toolCallDisplay'

export type BackgroundJobsPanelItem =
  | {
      kind: 'host'
      key: string
      title: string
      jobKind: string
      /** JobSupervisor job id. */
      jobId: string
    }
  | {
      kind: 'await'
      key: string
      toolCall: ToolCall
      title?: undefined
      jobKind?: undefined
      jobId: null
    }

const RUNNING_STATUSES: ReadonlySet<ToolCall['status']> = new Set([
  'running',
  'pending'
])

export function occupancyJobTitle(job: BackgroundJobView): string {
  const title = job.title?.trim()
  if (title) return title
  if (job.kind === 'terminal') return t('backgroundJobs.kind.terminal')
  return t('backgroundJobs.kind.subtask')
}

/**
 * Composer 后台条：占用名单只认 JobSupervisor（含嵌套后台终端）。
 * `job.await` 不是 job，从当前消息补一行。
 */
export function collectBackgroundJobsPanelItems(
  occupancy: readonly BackgroundJobView[] | undefined,
  messages: readonly ChatMessage[] | undefined
): BackgroundJobsPanelItem[] {
  const hosts = (occupancy ?? []).map(job => ({
    kind: 'host' as const,
    key: job.jobId,
    title: occupancyJobTitle(job),
    jobKind: job.kind,
    jobId: job.jobId
  }))
  return [...hosts, ...collectLiveAwaitJobs(messages)]
}

function collectLiveAwaitJobs(
  messages: readonly ChatMessage[] | undefined
): Extract<BackgroundJobsPanelItem, { kind: 'await' }>[] {
  const seen = new Set<string>()
  const out: Extract<BackgroundJobsPanelItem, { kind: 'await' }>[] = []
  for (const message of messages ?? []) {
    for (const tc of message.toolCalls ?? []) {
      if (!RUNNING_STATUSES.has(tc.status)) continue
      if (seen.has(tc.id)) continue
      if (!isJobAwaitCall(tc)) continue
      seen.add(tc.id)
      out.push({ kind: 'await', key: tc.id, toolCall: tc, jobId: null })
    }
  }
  return out
}
