import { t } from '../i18n'
import { toolCallBaseName } from './messageTooling'
import type { AgentTrace, SubAgentToolStats } from '../types/chat'

export function emptySubAgentToolStats(): SubAgentToolStats {
  return {
    searchCount: 0,
    readCount: 0,
    writeCount: 0,
    terminalCount: 0,
    webSearchCount: 0,
    skillCount: 0,
    mediaCount: 0,
    jobCount: 0,
    delegateCount: 0,
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
  if (base === 'file_read') {
    stats.readCount += 1
    return
  }
  if (base === 'file_write' || base === 'file_edit') {
    stats.writeCount = (stats.writeCount ?? 0) + 1
    return
  }
  if (base === 'file_grep' || base === 'file_glob' || base === 'file_list') {
    stats.searchCount += 1
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
    return
  }
  if (base === 'job') {
    stats.jobCount = (stats.jobCount ?? 0) + 1
    return
  }
  if (base === 'run_subagent') {
    stats.delegateCount = (stats.delegateCount ?? 0) + 1
    return
  }
  // Everything the buckets above do not name still counts: interaction / orchestration
  // tools (ask_user, task_board_*, read_lints …). Without this a spawn that only asked a
  // question reported「工具 0 次」and collapsed to「过程」.
  stats.otherCount = (stats.otherCount ?? 0) + 1
}

export function subAgentStatusLabel(status: string): string {
  if (status === 'completed') return t('tools.subAgentCompleted')
  if (status === 'failed') return t('tools.subAgentFailed')
  if (status === 'running') return t('tools.subAgentRunning')
  return status
}

function statSeg(label: string, count: number | undefined): string | null {
  const n = count ?? 0
  return n > 0 ? t('tools.statSeg', { label, count: n }) : null
}

function joinStatSegments(parts: Array<string | null>): string {
  const visible = parts.filter((p): p is string => !!p)
  return visible.length > 0 ? visible.join(' · ') : t('tools.statEmpty')
}

type CounterKey = keyof SubAgentToolStats

/**
 * 统计口径对所有智能体一致：每个计数器登记一次标签，按固定顺序输出。
 *
 * 以前是每个 agent 一份白名单（computer / coder / general-worker / explore 各挑几个桶）。
 * computer 那份漏了终端等 7 个桶，于是「其他 2 次」旁边列着 5 条工具。改成一张表后：
 * 同一个词只对应一个计数器（「检索」= 文件检索，「联网」= web_search），也不会再漏。
 *
 * 新增计数器时忘了登记，`subAgentStats.test.ts` 的覆盖断言会红。
 */
export const SUB_AGENT_STAT_BUCKETS = [
  ['mouseCount', 'tools.statMouse'],
  ['inputCount', 'tools.statInput'],
  ['terminalCount', 'tools.statTerminal'],
  ['searchCount', 'tools.statSearch'],
  ['readCount', 'tools.statRead'],
  ['writeCount', 'tools.statEdit'],
  ['webSearchCount', 'tools.statWeb'],
  ['skillCount', 'tools.statSkill'],
  ['mediaCount', 'tools.statMedia'],
  ['jobCount', 'tools.statJob'],
  ['delegateCount', 'tools.statDelegate'],
  ['otherCount', 'tools.statOther']
] as const satisfies ReadonlyArray<readonly [CounterKey, string]>

/** 行内数字之和 == 工具调用数：这张表覆盖 `SubAgentToolStats` 的每个计数器。 */
function formatStats(stats: SubAgentToolStats): string {
  return joinStatSegments(
    SUB_AGENT_STAT_BUCKETS.map(([key, labelKey]) => statSeg(t(labelKey), stats[key] ?? 0))
  )
}

/** Collapsed process row when scoped stats are not hydrated yet. */
export function subAgentProcessPlaceholder(): string {
  return t('tools.subAgentProcess')
}

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
export function formatSubAgentStatsLine(status: string, stats: SubAgentToolStats): string {
  const metrics = formatStats(stats)
  if (status === 'failed') {
    return `${metrics} · ${subAgentStatusLabel(status)}`
  }
  return metrics
}

export function formatSubAgentSummaryLine(
  name: string,
  status: string,
  stats: SubAgentToolStats
): string {
  const label = name.trim() || t('tools.subAgentFallbackName')
  return `${label} · ${formatSubAgentStatsLine(status, stats)}`
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
  liveToolLine?: string | null
  thinkingLine?: string | null
}): CollapsedSubAgentView {
  const metrics = formatStats(input.stats)
  const hasStats = input.status === 'failed' || metrics !== t('tools.statEmpty')
  const statsLine = hasStats ? formatSubAgentStatsLine(input.status, input.stats) : ''
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
