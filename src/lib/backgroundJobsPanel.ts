import type { ChatMessage, ToolCall } from '../types/chat'
import {
  backgroundJobIdFromToolCall,
  isBackgroundJobHost,
  isJobAwaitCall
} from './toolCallDisplay'

export type BackgroundJobsPanelItem =
  | {
      kind: 'host'
      key: string
      toolCall: ToolCall
      /** 可取消的后台任务 id；host 尚未拿到 jobId 时为 null。 */
      jobId: string | null
    }
  | {
      kind: 'await'
      key: string
      toolCall: ToolCall
      /** await 工具自身没有独立 jobId。 */
      jobId: null
    }

const RUNNING_STATUSES: ReadonlySet<ToolCall['status']> = new Set([
  'running',
  'pending'
])

/**
 * 扫描当前会话消息里仍在运行的后台任务宿主（terminal / run_subagent 等
 * background 工具）与「等待后台任务」的 await 工具，去重后供
 * BackgroundJobsPanel 逐条展示。数量上限仍以 chat.backgroundJobCount 为准。
 */
export function collectLiveBackgroundJobs(
  messages: readonly ChatMessage[] | undefined
): BackgroundJobsPanelItem[] {
  const seen = new Set<string>()
  const out: BackgroundJobsPanelItem[] = []
  for (const message of messages ?? []) {
    for (const tc of message.toolCalls ?? []) {
      if (!RUNNING_STATUSES.has(tc.status)) continue
      if (seen.has(tc.id)) continue
      const jobId = backgroundJobIdFromToolCall(tc)
      if (isBackgroundJobHost(tc)) {
        seen.add(tc.id)
        out.push({ kind: 'host', key: tc.id, toolCall: tc, jobId })
      } else if (isJobAwaitCall(tc)) {
        seen.add(tc.id)
        out.push({ kind: 'await', key: tc.id, toolCall: tc, jobId: null })
      }
    }
  }
  return out
}
