import { toolCallBaseName } from './messageTooling'
import type { SubAgentToolStats } from '../types/chat'

export function emptySubAgentToolStats(): SubAgentToolStats {
  return {
    searchCount: 0,
    readCount: 0,
    writeCount: 0,
    terminalCount: 0,
    webSearchCount: 0,
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

function desktopToolFamily(base: string): 'mouse' | 'input' | 'other' | null {
  if (base.startsWith('mouse_')) return 'mouse'
  if (base.startsWith('input_')) return 'input'
  if (
    base === 'hotkey' ||
    base === 'wait' ||
    base === 'action_verify' ||
    base.startsWith('clipboard_') ||
    base.startsWith('modified_click_') ||
    base.startsWith('captcha_verify_')
  ) {
    return 'other'
  }
  return null
}

/** Count successful sub-agent tool invocations (agent-agnostic buckets). */
export function incrementSubAgentToolStats(
  stats: SubAgentToolStats,
  toolName: string,
  _argsJson?: string
): void {
  const base = toolCallBaseName(toolName.trim())
  const desktop = desktopToolFamily(base)
  if (desktop === 'mouse') {
    stats.mouseCount = (stats.mouseCount ?? 0) + 1
    return
  }
  if (desktop === 'input') {
    stats.inputCount = (stats.inputCount ?? 0) + 1
    return
  }
  if (desktop === 'other') {
    stats.otherCount = (stats.otherCount ?? 0) + 1
    return
  }
  if (base.startsWith('file_')) {
    if (base === 'file_read') stats.readCount += 1
    else if (base === 'file_write' || base === 'file_edit') {
      stats.writeCount = (stats.writeCount ?? 0) + 1
    } else if (base === 'file_grep' || base === 'file_glob' || base === 'file_list') {
      stats.searchCount += 1
    }
    return
  }
  if (base === 'terminal') {
    stats.terminalCount = (stats.terminalCount ?? 0) + 1
    return
  }
  if (base === 'web_search') {
    stats.webSearchCount = (stats.webSearchCount ?? 0) + 1
  }
}

export function subAgentStatusLabel(status: string): string {
  if (status === 'completed') return '已完成'
  if (status === 'failed') return '失败'
  if (status === 'running') return '进行中'
  return status
}

function statSeg(label: string, count: number | undefined): string | null {
  const n = count ?? 0
  return n > 0 ? `${label} ${n} 次` : null
}

function joinStatSegments(parts: Array<string | null>): string {
  const visible = parts.filter((p): p is string => !!p)
  return visible.length > 0 ? visible.join(' · ') : '工具 0 次'
}

function formatStatsForAgent(agentId: string, stats: SubAgentToolStats): string {
  const id = agentId.trim().toLowerCase()
  if (id === 'computer') {
    return joinStatSegments([
      statSeg('鼠标', stats.mouseCount),
      statSeg('输入', stats.inputCount),
      statSeg('其他', stats.otherCount)
    ])
  }
  if (id === 'research') {
    return joinStatSegments([statSeg('联网搜索', stats.webSearchCount)])
  }
  if (id === 'coder') {
    return joinStatSegments([
      statSeg('搜索', stats.searchCount),
      statSeg('读文件', stats.readCount),
      statSeg('终端', stats.terminalCount),
      statSeg('编辑', stats.writeCount)
    ])
  }
  // explore and unknown file-heavy workers
  return joinStatSegments([
    statSeg('搜索', stats.searchCount),
    statSeg('读文件', stats.readCount)
  ])
}

export function formatSubAgentSummaryLine(
  name: string,
  status: string,
  stats: SubAgentToolStats,
  agentId?: string
): string {
  const label = name.trim() || '子任务'
  const metrics = formatStatsForAgent(agentId ?? 'explore', stats)
  return `${label} · ${subAgentStatusLabel(status)} · ${metrics}`
}
