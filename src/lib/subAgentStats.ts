import { toolCallBaseName } from './messageTooling'
import type { AgentTrace, SubAgentToolStats } from '../types/chat'
import { t } from '../i18n'

export function emptySubAgentToolStats(): SubAgentToolStats {
  return {
    searchCount: 0,
    readCount: 0,
    writeCount: 0,
    terminalCount: 0,
    webSearchCount: 0,
    skillCount: 0,
    mediaCount: 0,
    mouseCount: 0,
    inputCount: 0,
    otherCount: 0
  }
}

/** Legacy or instance-scoped trace id → agent id suffix. */
export function subAgentIdFromTraceId(traceId: string): string {
  const i = traceId.lastIndexOf(':')
  return i > 0 ? traceId.slice(i + 1).trim() : ''
}

/** Legacy or instance-scoped trace id → task id prefix. */
export function subTaskIdFromTraceId(traceId: string): string {
  const i = traceId.indexOf(':')
  return i > 0 ? traceId.slice(0, i).trim() : traceId.trim()
}

/**
 * Middle segment of self-fork traces `taskId:instanceId:agentId`.
 * Legacy `taskId:agentId` has no instance → null.
 */
export function agentInstanceIdFromTraceId(traceId: string): string | null {
  const parts = traceId
    .split(':')
    .map(s => s.trim())
    .filter(Boolean)
  if (parts.length < 3) return null
  return parts[1] ?? null
}

export function resolveTraceAgentId(
  trace: Pick<AgentTrace, 'id' | 'agentId'>
): string {
  return trace.agentId?.trim() || subAgentIdFromTraceId(trace.id)
}

export function resolveTraceTaskId(
  trace: Pick<AgentTrace, 'id' | 'taskId'>
): string {
  const explicit = trace.taskId?.trim()
  if (explicit) return explicit
  if (!trace.id.includes(':')) return ''
  return subTaskIdFromTraceId(trace.id)
}

/**
 * Lookup key for task-board / parsers that still expect `task:agent` or
 * `task:instance:agent`. New traces use SpawnId as `id`.
 */
export function traceLookupId(
  trace: Pick<AgentTrace, 'id' | 'taskId' | 'agentId' | 'agentInstanceId'>
): string {
  if (trace.id.includes(':')) return trace.id
  const task = resolveTraceTaskId(trace)
  const agent = resolveTraceAgentId(trace)
  const inst = trace.agentInstanceId?.trim() || trace.id
  if (task && agent && inst) return `${task}:${inst}:${agent}`
  if (task && agent) return `${task}:${agent}`
  return trace.id
}

function desktopToolFamily(base: string): 'mouse' | 'input' | 'other' | null {
  if (base.startsWith('mouse_')) return 'mouse'
  if (base.startsWith('input_')) return 'input'
  if (
    base === 'launch_app' ||
    base === 'list_apps' ||
    base === 'hotkey' ||
    base === 'wait' ||
    base.startsWith('clipboard_') ||
    base.startsWith('modified_click_')
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
  if (base === 'skill_read' || base === 'skill_import') {
    stats.skillCount = (stats.skillCount ?? 0) + 1
    return
  }
  if (base === 'media_understand' || base === 'image_generate' || base === 'video_generate') {
    stats.mediaCount = (stats.mediaCount ?? 0) + 1
    return
  }
  if (base === 'session_search' || base === 'session_read' || base === 'memory') {
    stats.searchCount += 1
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
  if (status === 'completed') return t('subAgent.statusCompleted')
  if (status === 'failed') return t('subAgent.statusFailed')
  if (status === 'running') return t('subAgent.statusRunning')
  return status
}

function statSeg(label: string, count: number | undefined): string | null {
  const n = count ?? 0
  return n > 0 ? t('subAgent.statCount', { label, n }) : null
}

function joinStatSegments(parts: Array<string | null>): string {
  const visible = parts.filter((p): p is string => !!p)
  return visible.length > 0 ? visible.join(' · ') : t('subAgent.toolsZero')
}

function formatStatsForAgent(agentId: string, stats: SubAgentToolStats): string {
  const id = agentId.trim().toLowerCase()
  if (id === 'computer') {
    return joinStatSegments([
      statSeg(t('subAgent.statMouse'), stats.mouseCount),
      statSeg(t('subAgent.statInput'), stats.inputCount),
      statSeg(t('subAgent.statOther'), stats.otherCount)
    ])
  }
  if (id === 'coder') {
    return joinStatSegments([
      statSeg(t('subAgent.statSearch'), stats.searchCount),
      statSeg(t('subAgent.statRead'), stats.readCount),
      statSeg(t('subAgent.statTerminal'), stats.terminalCount),
      statSeg(t('subAgent.statEdit'), stats.writeCount)
    ])
  }
  if (id === 'general-worker' || id === 'general_worker') {
    return joinStatSegments([
      statSeg(t('subAgent.statTerminal'), stats.terminalCount),
      statSeg(t('subAgent.statSkill'), stats.skillCount),
      statSeg(t('subAgent.statMedia'), stats.mediaCount),
      statSeg(t('subAgent.statSearch'), stats.webSearchCount),
      statSeg(t('subAgent.statRead'), stats.readCount),
      statSeg(t('subAgent.statEdit'), stats.writeCount)
    ])
  }
  // explore, self-fork (`current-agent`), and other file-heavy workers
  return joinStatSegments([
    statSeg(t('subAgent.statSearch'), stats.searchCount),
    statSeg(t('subAgent.statRead'), stats.readCount),
    statSeg(t('subAgent.statTerminal'), stats.terminalCount),
    statSeg(t('subAgent.statEdit'), stats.writeCount)
  ])
}

/** Collapsed process row when scoped stats are not hydrated yet. */
export function subAgentProcessPlaceholder(): string {
  return t('subAgent.processPlaceholder')
}

/** @deprecated Prefer subAgentProcessPlaceholder(). */
export const SUB_AGENT_PROCESS_PLACEHOLDER = subAgentProcessPlaceholder()

/**
 * Stats line under the host row. While the spawn is running and a live /
 * thinking line already fills the current-task slot, keep the summary blank
 * (parent first-round parity) instead of a barren「过程」placeholder.
 */
export function resolveSubAgentSummaryDisplay(input: {
  statsSummary: string
  running: boolean
  liveLine?: string | null
}): string {
  const stats = input.statsSummary.trim()
  if (stats) return stats
  if (input.running && (input.liveLine ?? '').trim()) return ''
  return subAgentProcessPlaceholder()
}

/** Counts only — host「委派子任务」row already shows the goal. */
export function formatSubAgentStatsLine(
  status: string,
  stats: SubAgentToolStats,
  agentId?: string
): string {
  const metrics = formatStatsForAgent(agentId ?? 'explore', stats)
  if (status === 'failed') {
    return `${metrics} · ${subAgentStatusLabel(status)}`
  }
  return metrics
}

export function formatSubAgentSummaryLine(
  name: string,
  status: string,
  stats: SubAgentToolStats,
  agentId?: string
): string {
  const label = name.trim() || t('subAgent.defaultName')
  return `${label} · ${formatSubAgentStatsLine(status, stats, agentId)}`
}

export type CollapsedSubAgentView = {
  summaryLine: string
  liveLine: string | null
}

/**
 * Stats + live lines under the host「委派子任务」row.
 * Orphan frames (no host row) still prefix the goal on the stats line.
 * Background status stays on the host tool row only — do not repeat「后台执行中」here.
 */
export function resolveCollapsedSubAgentView(input: {
  orphanTitle?: string
  status: string
  stats: SubAgentToolStats
  agentId?: string
  liveToolLine?: string | null
  thinkingLine?: string | null
}): CollapsedSubAgentView {
  const metrics = formatStatsForAgent(input.agentId ?? 'explore', input.stats)
  const hasStats = input.status === 'failed' || metrics !== t('subAgent.toolsZero')
  let statsLine = hasStats
    ? formatSubAgentStatsLine(input.status, input.stats, input.agentId)
    : ''
  const orphan = input.orphanTitle?.trim() || ''
  const summaryLine = orphan
    ? (statsLine ? `${orphan} · ${statsLine}` : orphan)
    : statsLine

  const live = input.liveToolLine?.trim() || ''
  const thinking = input.thinkingLine?.trim() || ''
  if (thinking && !live) {
    return { summaryLine, liveLine: thinking }
  }
  if (live) {
    return { summaryLine, liveLine: live }
  }
  return { summaryLine, liveLine: null }
}
