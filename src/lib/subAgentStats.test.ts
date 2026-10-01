import { describe, expect, it } from 'vitest'
import {
  agentInstanceIdFromTraceId,
  emptySubAgentToolStats,
  formatSubAgentStatsLine,
  formatSubAgentSummaryLine,
  incrementSubAgentToolStats,
  resolveCollapsedSubAgentView,
  resolveSubAgentSummaryDisplay,
  SUB_AGENT_PROCESS_PLACEHOLDER,
  subAgentIdFromTraceId,
  subTaskIdFromTraceId
} from './subAgentStats'

describe('sub-agent trace identity parsing', () => {
  it('preserves task and agent ids for instance-scoped self-fork traces', () => {
    const traceId = 'task-reused:instance-unique:current-agent'

    expect(subTaskIdFromTraceId(traceId)).toBe('task-reused')
    expect(agentInstanceIdFromTraceId(traceId)).toBe('instance-unique')
    expect(subAgentIdFromTraceId(traceId)).toBe('current-agent')
  })

  it('keeps parsing legacy registered-agent traces', () => {
    const traceId = 'task-legacy:explore'

    expect(subTaskIdFromTraceId(traceId)).toBe('task-legacy')
    expect(agentInstanceIdFromTraceId(traceId)).toBeNull()
    expect(subAgentIdFromTraceId(traceId)).toBe('explore')
  })
})

describe('subAgentStats general-worker', () => {
  it('counts skill, terminal, and media tools', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'skill_read', '{}')
    incrementSubAgentToolStats(stats, 'skill_read', '{}')
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'media_understand', '{}')
    incrementSubAgentToolStats(stats, 'file_read', '{}')

    const line = formatSubAgentSummaryLine('通用执行', 'completed', stats, 'general-worker')
    expect(line).toBe('通用执行 · 终端 1 次 · 技能 2 次 · 媒体 1 次 · 读文件 1 次')
  })

  it('does not fall back to explore-only buckets for general-worker', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    const line = formatSubAgentSummaryLine('通用执行', 'completed', stats, 'general-worker')
    expect(line).toContain('终端 2 次')
    expect(line).not.toBe('通用执行 · 工具 0 次')
  })
})

describe('subAgentStats explore / self-fork summary', () => {
  it('includes terminal commands for explore', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'file_read', '{}')
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    const line = formatSubAgentSummaryLine('代码探索', 'completed', stats, 'explore')
    expect(line).toBe('代码探索 · 读文件 1 次 · 终端 2 次')
  })

  it('folds unnamed tools into「其他」so interaction-only spawns stay visible', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'ask_user', '{}')
    incrementSubAgentToolStats(stats, 'task_board_patch', '{}')
    expect(formatSubAgentSummaryLine('代码探索', 'completed', stats, 'explore'))
      .toBe('代码探索 · 其他 2 次')
  })

  it('keeps 失败 on failed traces and omits 已完成 on success', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'file_read', '{}')
    expect(formatSubAgentSummaryLine('代码探索', 'failed', stats, 'explore')).toBe(
      '代码探索 · 读文件 1 次 · 失败'
    )
    expect(formatSubAgentSummaryLine('代码探索', 'completed', stats, 'explore')).not.toContain('已完成')
  })

  it('omits 进行中 / 执行中 while the sub-agent is still running', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'file_read', '{}')
    const line = formatSubAgentSummaryLine('代码探索', 'running', stats, 'explore')
    expect(line).toBe('代码探索 · 读文件 1 次')
    expect(line).not.toContain('进行中')
    expect(line).not.toContain('执行中')
  })

  it('includes terminal for self-fork current-agent traces', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    const line = formatSubAgentSummaryLine('当前 Agent', 'completed', stats, 'current-agent')
    expect(line).toContain('终端 1 次')
  })
})

describe('formatSubAgentStatsLine', () => {
  it('omits the task goal so the host row can own it', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'file_read', '{}')
    expect(formatSubAgentStatsLine('completed', stats, 'explore')).toBe('读文件 1 次')
    expect(formatSubAgentStatsLine('failed', stats, 'explore')).toBe('读文件 1 次 · 失败')
  })
})

describe('resolveCollapsedSubAgentView', () => {
  const stats = emptySubAgentToolStats()
  incrementSubAgentToolStats(stats, 'terminal', '{}')

  it('uses finished stats without repeating the task goal', () => {
    const view = resolveCollapsedSubAgentView({
      status: 'completed',
      stats,
      agentId: 'general-worker'
    })
    expect(view.summaryLine).toBe('终端 1 次')
    expect(view.liveLine).toBeNull()
  })

  it('parks the live inner tool when no counts exist yet', () => {
    const view = resolveCollapsedSubAgentView({
      status: 'running',
      stats: emptySubAgentToolStats(),
      agentId: 'general-worker',
      liveToolLine: '终端 · df -h'
    })
    expect(view.summaryLine).toBe('')
    expect(view.liveLine).toBe('终端 · df -h')
  })

  it('keeps finished stats on the summary and parks the current inner tool below', () => {
    const view = resolveCollapsedSubAgentView({
      status: 'running',
      stats,
      agentId: 'general-worker',
      liveToolLine: '终端 · du -sh'
    })
    expect(view.summaryLine).toBe('终端 1 次')
    expect(view.liveLine).toBe('终端 · du -sh')
  })

  it('keeps thinking on the live line before any inner tool finishes', () => {
    const view = resolveCollapsedSubAgentView({
      status: 'running',
      stats: emptySubAgentToolStats(),
      thinkingLine: '思考中..'
    })
    expect(view.summaryLine).toBe('')
    expect(view.liveLine).toBe('思考中..')
  })

  it('puts thinking on the live line after finished inner work', () => {
    const view = resolveCollapsedSubAgentView({
      status: 'running',
      stats,
      agentId: 'general-worker',
      thinkingLine: '思考中..'
    })
    expect(view.summaryLine).toBe('终端 1 次')
    expect(view.liveLine).toBe('思考中..')
  })

  it('prefixes the goal only for orphan frames without a host row', () => {
    const view = resolveCollapsedSubAgentView({
      orphanTitle: '系统信息探测',
      status: 'completed',
      stats,
      agentId: 'general-worker'
    })
    expect(view.summaryLine).toBe('系统信息探测 · 终端 1 次')
  })

  it('keeps collapsed stats as tool counts only while background host is running', () => {
    const empty = resolveCollapsedSubAgentView({
      status: 'running',
      stats: emptySubAgentToolStats(),
      agentId: 'explore'
    })
    expect(empty.summaryLine).toBe('')

    const withStats = resolveCollapsedSubAgentView({
      status: 'running',
      stats,
      agentId: 'explore'
    })
    expect(withStats.summaryLine).toBe('终端 1 次')
  })
})

describe('resolveSubAgentSummaryDisplay', () => {
  it('keeps real stats', () => {
    expect(
      resolveSubAgentSummaryDisplay({
        statsSummary: '搜索 1 次 · 终端 16 次',
        running: true,
        liveLine: '思考中..'
      })
    ).toBe('搜索 1 次 · 终端 16 次')
  })

  it('avoids「过程」when a running spawn already has a live line', () => {
    expect(
      resolveSubAgentSummaryDisplay({
        statsSummary: '',
        running: true,
        liveLine: '思考中.........'
      })
    ).toBe('')
  })

  it('uses「过程」when idle / not yet hydrated and there is no live line', () => {
    expect(
      resolveSubAgentSummaryDisplay({
        statsSummary: '',
        running: false,
        liveLine: null
      })
    ).toBe(SUB_AGENT_PROCESS_PLACEHOLDER)
  })
})
