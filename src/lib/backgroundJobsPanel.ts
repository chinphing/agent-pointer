import type { BackgroundJobView } from '../types/chat'
import { t } from '../i18n'

export type BackgroundJobsPanelItem = {
  kind: 'host'
  key: string
  title: string
  jobKind: string
  /** JobSupervisor job id. */
  jobId: string
}

export function occupancyJobTitle(job: BackgroundJobView): string {
  const title = job.title?.trim()
  if (title) return title
  if (job.kind === 'terminal') return t('chat.terminalJob')
  return t('chat.subtaskJob')
}

/**
 * Composer 后台条：只列 JobSupervisor 占用名单里的真实 job。
 * `job.await` 不是 job（它的「结束等待」按钮在消息里的工具行上），
 * 因此行数恒等于 `backgroundJobCount`，徽标与列表不会对不上。
 */
export function collectBackgroundJobsPanelItems(
  occupancy: readonly BackgroundJobView[] | undefined
): BackgroundJobsPanelItem[] {
  return (occupancy ?? []).map(job => ({
    kind: 'host' as const,
    key: job.jobId,
    title: occupancyJobTitle(job),
    jobKind: job.kind,
    jobId: job.jobId
  }))
}
