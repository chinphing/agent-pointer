import { describe, expect, it } from 'vitest'
import {
  emptySubAgentToolStats,
  formatSubAgentSummaryLine,
  incrementSubAgentToolStats,
  subAgentIdFromTraceId,
  subTaskIdFromTraceId
} from './subAgentStats'

describe('sub-agent trace identity parsing', () => {
  it('preserves task and agent ids for instance-scoped self-fork traces', () => {
    const traceId = 'task-reused:instance-unique:current-agent'

    expect(subTaskIdFromTraceId(traceId)).toBe('task-reused')
    expect(subAgentIdFromTraceId(traceId)).toBe('current-agent')
  })

  it('keeps parsing legacy registered-agent traces', () => {
    const traceId = 'task-legacy:explore'

    expect(subTaskIdFromTraceId(traceId)).toBe('task-legacy')
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
    expect(line).toBe('通用执行 · 已完成 · 终端 1 次 · 技能 2 次 · 媒体 1 次 · 读文件 1 次')
  })

  it('does not fall back to explore-only buckets for general-worker', () => {
    const stats = emptySubAgentToolStats()
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    incrementSubAgentToolStats(stats, 'terminal', '{}')
    const line = formatSubAgentSummaryLine('通用执行', 'completed', stats, 'general-worker')
    expect(line).toContain('终端 2 次')
    expect(line).not.toBe('通用执行 · 已完成 · 工具 0 次')
  })
})
