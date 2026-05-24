import { toolCallBaseName } from './messageTooling'
import type { SubAgentToolStats } from '../types/chat'

export function emptySubAgentToolStats(): SubAgentToolStats {
  return { searchCount: 0, readCount: 0 }
}

/** Count successful sub-agent tool invocations for collapsed summary line. */
export function incrementSubAgentToolStats(stats: SubAgentToolStats, toolName: string): void {
  const base = toolCallBaseName(toolName.trim())
  if (base === 'file') {
    const method = toolName.includes(':') ? toolName.split(':')[1]?.trim() : ''
    if (method === 'read') stats.readCount += 1
    else if (method === 'grep' || method === 'glob' || method === 'list') stats.searchCount += 1
    return
  }
  if (base === 'grep' || base === 'glob' || base === 'list') stats.searchCount += 1
}

export function subAgentStatusLabel(status: string): string {
  if (status === 'completed') return '已完成'
  if (status === 'failed') return '失败'
  if (status === 'running') return '进行中'
  return status
}

export function formatSubAgentSummaryLine(
  name: string,
  status: string,
  stats: SubAgentToolStats
): string {
  const label = name.trim() || '子任务'
  return `${label} · ${subAgentStatusLabel(status)} · 搜索 ${stats.searchCount} 次 · 读文件 ${stats.readCount} 次`
}
