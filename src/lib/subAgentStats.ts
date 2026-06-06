import { toolCallBaseName } from './messageTooling'
import type { SubAgentToolStats } from '../types/chat'

export function emptySubAgentToolStats(): SubAgentToolStats {
  return {
    searchCount: 0,
    readCount: 0,
    mouseCount: 0,
    inputCount: 0,
    otherCount: 0
  }
}

/** `{taskId}:{agentId}` → agent id suffix. */
export function subAgentIdFromTraceId(traceId: string): string {
  const i = traceId.indexOf(':')
  return i > 0 ? traceId.slice(i + 1).trim() : ''
}

/** `{taskId}:{agentId}` → task id prefix. */
export function subTaskIdFromTraceId(traceId: string): string {
  const i = traceId.indexOf(':')
  return i > 0 ? traceId.slice(0, i).trim() : traceId.trim()
}

function isDesktopTool(base: string): boolean {
  if (base.startsWith('mouse_') || base.startsWith('input_')) return true
  return (
    base === 'hotkey' ||
    base === 'wait' ||
    base === 'action_verify' ||
    base.startsWith('clipboard_') ||
    base.startsWith('modified_click_') ||
    base.startsWith('captcha_verify_')
  )
}

/** Count successful sub-agent tool invocations for collapsed summary line. */
export function incrementSubAgentToolStats(
  stats: SubAgentToolStats,
  toolName: string,
  _argsJson?: string
): void {
  const base = toolCallBaseName(toolName.trim())
  if (isDesktopTool(base)) {
    stats.readCount += 1
    return
  }
  if (base.startsWith('file_')) {
    if (base === 'file_read') stats.readCount += 1
    else if (base === 'file_grep' || base === 'file_glob' || base === 'file_list') stats.searchCount += 1
    return
  }
  if (base === 'web_search') stats.searchCount += 1
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
