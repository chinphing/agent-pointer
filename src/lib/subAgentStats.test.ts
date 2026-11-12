import { describe, expect, it } from 'vitest'
import { i18n } from '../i18n'
import {
  agentInstanceIdFromTraceId,
  emptySubAgentToolStats,
  formatSubAgentStatsLine,
  formatSubAgentSummaryLine,
  incrementSubAgentToolStats,
  resolveCollapsedSubAgentView,
  resolveSubAgentSummaryDisplay,
  subAgentProcessPlaceholder,
  SUB_AGENT_STAT_BUCKETS,
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

    const line = formatSubAgentSummaryLine('通用执行', 'completed', stats)
    expect(line).toBe('通用执行 · 终端 1 次 · 读文件 1 次 · 技能 2 次 · 媒体 1 次')
  })

  it('does not fall back to explore-only buckets for general-worker', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    const line = formatSubAgentSummaryLine('通用执行', 'completed', stats)
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
    const line = formatSubAgentSummaryLine('代码探索', 'completed', stats)
    expect(line).toBe('代码探索 · 终端 2 次 · 读文件 1 次')
  })

  it('folds unnamed tools into「其他」so interaction-only spawns stay visible', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'ask_user', '{}')
    incrementSubAgentToolStats(stats, 'task_board_patch', '{}')
    expect(formatSubAgentSummaryLine('代码探索', 'completed', stats))
      .toBe('代码探索 · 其他 2 次')
  })

  it('keeps background jobs and re-delegation in their own buckets', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'job', '{}')
    incrementSubAgentToolStats(stats, 'run_subagent', '{}')
    incrementSubAgentToolStats(stats, 'ask_user', '{}')
    expect(formatSubAgentSummaryLine('代码探索', 'completed', stats))
      .toBe('代码探索 · 后台任务 1 次 · 委派 1 次 · 其他 1 次')
  })

  it('keeps 失败 on failed traces and omits 已完成 on success', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'file_read', '{}')
    expect(formatSubAgentSummaryLine('代码探索', 'failed', stats)).toBe(
      '代码探索 · 读文件 1 次 · 失败'
    )
    expect(formatSubAgentSummaryLine('代码探索', 'completed', stats)).not.toContain('已完成')
  })

  it('omits 进行中 / 执行中 while the sub-agent is still running', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'file_read', '{}')
    const line = formatSubAgentSummaryLine('代码探索', 'running', stats)
    expect(line).toBe('代码探索 · 读文件 1 次')
    expect(line).not.toContain('进行中')
    expect(line).not.toContain('执行中')
  })

  it('includes terminal for self-fork current-agent traces', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    const line = formatSubAgentSummaryLine('当前 Agent', 'completed', stats)
    expect(line).toContain('终端 1 次')
  })
})

describe('subAgentStats 数字必须与工具行对得上账', () => {
  it('computer 把终端单列，不再吞进「其他」', () => {
    // 真实案例：子任务先跑 3 条终端命令查默认浏览器，再 list_apps + launch_app。
    // 旧口径只显示「其他 2 次」，旁边却列着 5 条工具。
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'list_apps', '{}')
    incrementSubAgentToolStats(stats, 'launch_app', '{}')

    expect(formatSubAgentStatsLine('completed', stats))
      .toBe('终端 3 次 · 其他 2 次')
  })

  it('任何工具组合下，行内数字之和都等于工具调用数', () => {
    const toolSets: string[][] = [
      ['mouse_click_at', 'input_at', 'terminal', 'hotkey', 'list_apps', 'media_understand', 'task_board_patch'],
      ['file_grep', 'file_read', 'file_edit', 'terminal', 'web_search', 'skill_read', 'job', 'run_subagent', 'ask_user'],
      ['terminal', 'skill_read', 'media_understand', 'web_search', 'file_read', 'file_write', 'job', 'run_subagent', 'ask_user'],
      ['file_grep', 'file_read', 'terminal', 'file_write', 'web_search', 'media_understand', 'job', 'run_subagent', 'ask_user']
    ]

    for (const tools of toolSets) {
      const stats = emptySubAgentToolStats()
      for (const tool of tools) incrementSubAgentToolStats(stats, tool, '{}')
      const line = formatSubAgentStatsLine('completed', stats)
      const sum = [...line.matchAll(/(\d+) 次/g)].reduce((n, m) => n + Number(m[1]), 0)
      expect({ sum, line }).toEqual({ sum: tools.length, line })
    }
  })

  it('切到英文时标签与单位跟着变', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'list_apps', '{}')

    i18n.global.locale.value = 'en'
    try {
      expect(formatSubAgentStatsLine('completed', stats)).toBe('Terminal ×2 · Other ×1')
      expect(formatSubAgentStatsLine('failed', emptySubAgentToolStats())).toContain('Failed')
    } finally {
      i18n.global.locale.value = 'zh-CN'
    }

    expect(formatSubAgentStatsLine('completed', stats)).toBe('终端 2 次 · 其他 1 次')
  })

  it('每个计数器都登记了标签，不会再有算出来不显示的桶', () => {
    const registered = SUB_AGENT_STAT_BUCKETS.map(([key]) => key).sort()
    const counters = Object.keys(emptySubAgentToolStats()).sort()
    expect(registered).toEqual(counters)
  })

  it('同一个词只对应一个计数器：检索 = 文件检索，联网 = web_search', () => {
    const search = emptySubAgentToolStats()
    incrementSubAgentToolStats(search, 'file_grep', '{}')
    expect(formatSubAgentStatsLine('completed', search)).toBe('检索 1 次')

    const web = emptySubAgentToolStats()
    incrementSubAgentToolStats(web, 'web_search', '{}')
    expect(formatSubAgentStatsLine('completed', web)).toBe('联网 1 次')
  })
})

describe('formatSubAgentStatsLine', () => {
  it('omits the task goal so the host row can own it', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'file_read', '{}')
    expect(formatSubAgentStatsLine('completed', stats)).toBe('读文件 1 次')
    expect(formatSubAgentStatsLine('failed', stats)).toBe('读文件 1 次 · 失败')
  })
})

describe('resolveCollapsedSubAgentView', () => {
  const stats = emptySubAgentToolStats()
  incrementSubAgentToolStats(stats, 'terminal', '{}')

  it('uses finished stats without repeating the task goal', () => {
    const view = resolveCollapsedSubAgentView({
      status: 'completed',
      stats,
    })
    expect(view.summaryLine).toBe('终端 1 次')
    expect(view.liveLine).toBeNull()
  })

  it('parks the live inner tool when no counts exist yet', () => {
    const view = resolveCollapsedSubAgentView({
      status: 'running',
      stats: emptySubAgentToolStats(),
      liveToolLine: '终端 · df -h'
    })
    expect(view.summaryLine).toBe('')
    expect(view.liveLine).toBe('终端 · df -h')
  })

  it('keeps finished stats on the summary and parks the current inner tool below', () => {
    const view = resolveCollapsedSubAgentView({
      status: 'running',
      stats,
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
    })
    expect(view.summaryLine).toBe('系统信息探测 · 终端 1 次')
  })

  it('keeps collapsed stats as tool counts only while background host is running', () => {
    const empty = resolveCollapsedSubAgentView({
      status: 'running',
      stats: emptySubAgentToolStats(),
    })
    expect(empty.summaryLine).toBe('')

    const withStats = resolveCollapsedSubAgentView({
      status: 'running',
      stats,
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
    ).toBe(subAgentProcessPlaceholder())
  })
})
