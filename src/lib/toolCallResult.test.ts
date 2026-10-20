import { beforeEach, describe, expect, it } from 'vitest'
import type { ToolCall } from '../types/chat'
import {
  createJsonParseStats,
  installJsonParseProbe,
  type JsonParseStats
} from './jsonParseProbe'
import { resetToolCallFieldCache } from './toolCallFieldCache'
import { toolCallResultJson, toolCallResultObject, toolCallResultRaw } from './toolCallResult'
import {
  backgroundHandleStatus,
  backgroundJobIdFromToolCall,
  buildFileChangeSummaries,
  isBackgroundHandleInProgress,
  isBackgroundJobHandleResult,
  resolveBackgroundHostDisplayStatus
} from './toolCallDisplay'
import { taskBoardToolSummary } from './messageTooling'

beforeEach(() => {
  resetToolCallFieldCache()
})

/**
 * Run `body` with the real `JSON.parse` metered — the same probe the render-perf
 * HUD installs — so a test can assert how many parses one render path caused.
 */
function measureJsonParse(body: () => void): JsonParseStats {
  const stats = createJsonParseStats()
  const dispose = installJsonParseProbe({ stats })
  try {
    body()
  } finally {
    dispose()
  }
  return stats
}

function call(partial: Partial<ToolCall> & Pick<ToolCall, 'id' | 'name' | 'status'>): ToolCall {
  return { arguments: '', ...partial }
}

const HANDLE = JSON.stringify({ jobId: 'job_1', kind: 'terminal', status: 'running' })

describe('per-call result cache', () => {
  it('parses a result once for a whole render of a background host row', () => {
    const host = call({ id: 'host-1', name: 'run_subagent', status: 'success', result: HANDLE })

    const stats = measureJsonParse(() => {
      // What one `ToolCallRow` render asks for: display status, the template's
      // handle guard, the job id, and the handle status behind both.
      expect(resolveBackgroundHostDisplayStatus(host)).toBe('running')
      expect(isBackgroundJobHandleResult(host)).toBe(true)
      expect(backgroundJobIdFromToolCall(host)).toBe('job_1')
      expect(backgroundHandleStatus(host)).toBe('running')
      expect(isBackgroundHandleInProgress(host)).toBe(true)
    })

    expect(stats.calls).toBe(1)
    expect(stats.failed).toBe(0)
  })

  it('parses the same object + result once, then re-parses only after the result changes', () => {
    const tc = call({
      id: 'res-1',
      name: 'terminal',
      status: 'success',
      result: JSON.stringify({ stdout: 'first', exitCode: 0 })
    })

    const settled = measureJsonParse(() => {
      expect(toolCallResultObject(tc)?.stdout).toBe('first')
      expect(toolCallResultObject(tc)?.stdout).toBe('first')
      expect(toolCallResultObject(tc)?.stdout).toBe('first')
    })
    expect(settled.calls).toBe(1)

    tc.result = JSON.stringify({ stdout: 'second', exitCode: 0 })
    const changed = measureJsonParse(() => {
      expect(toolCallResultObject(tc)?.stdout).toBe('second')
      expect(toolCallResultObject(tc)?.stdout).toBe('second')
    })
    expect(changed.calls).toBe(1)
  })

  it('keeps two calls that share one result string independent', () => {
    const raw = JSON.stringify({ jobId: 'job_shared', kind: 'subagent', status: 'running' })
    const first = call({ id: 'share-a', name: 'run_subagent', status: 'success', result: raw })
    const second = call({ id: 'share-b', name: 'run_subagent', status: 'success', result: raw })

    const stats = measureJsonParse(() => {
      expect(backgroundJobIdFromToolCall(first)).toBe('job_shared')
      expect(backgroundJobIdFromToolCall(second)).toBe('job_shared')
      expect(backgroundHandleStatus(first)).toBe('running')
      expect(backgroundHandleStatus(second)).toBe('running')
    })

    // Per-object entries, never a value shared across rows: each object parses
    // its own copy once. Two rows cost two parses, not one, and not eight.
    expect(stats.calls).toBe(2)
  })

  it('parses a large result once no matter how often the row re-renders', () => {
    const big = JSON.stringify({ stdout: 'x'.repeat(200_000), stderr: '', exitCode: 0 })
    const tc = call({ id: 'big-1', name: 'terminal', status: 'success', result: big })

    const stats = measureJsonParse(() => {
      for (let i = 0; i < 20; i += 1) {
        expect(toolCallResultObject(tc)?.exitCode).toBe(0)
      }
    })

    expect(stats.calls).toBe(1)
    // The whole payload went through `JSON.parse` exactly once — this is the
    // HUD's "parse calls only grow on the first frame" check, in a test.
    expect(stats.totalChars).toBe(big.length)
  })

  it('drops the entry when the result is cleared', () => {
    const tc = call({
      id: 'cleared-1',
      name: 'terminal',
      status: 'success',
      result: JSON.stringify({ stdout: 'big output' })
    })
    expect(toolCallResultObject(tc)?.stdout).toBe('big output')

    delete tc.result
    const stats = measureJsonParse(() => {
      expect(toolCallResultObject(tc)).toBeNull()
      expect(isBackgroundJobHandleResult(tc)).toBe(false)
    })
    expect(stats.calls).toBe(0)
  })
})

describe('result parse outcomes', () => {
  it('parses an unparseable result once, not once per reader', () => {
    const tc = call({ id: 'text-1', name: 'terminal', status: 'success', result: 'plain text' })

    const stats = measureJsonParse(() => {
      expect(toolCallResultObject(tc)).toBeNull()
      expect(isBackgroundJobHandleResult(tc)).toBe(false)
      expect(backgroundHandleStatus(tc)).toBeNull()
      expect(taskBoardToolSummary(tc)).toBe('任务板已更新')
    })

    expect(stats.calls).toBe(1)
    expect(stats.failed).toBe(1)
  })

  it('never parses an absent result', () => {
    const empty = call({ id: 'empty-1', name: 'terminal', status: 'running', result: '' })
    const blank = call({ id: 'blank-1', name: 'terminal', status: 'running', result: '   ' })
    const missing = call({ id: 'missing-1', name: 'terminal', status: 'running' })

    const stats = measureJsonParse(() => {
      for (const tc of [empty, blank, missing]) {
        expect(toolCallResultRaw(tc)).not.toContain('{')
        expect(toolCallResultJson(tc).ok).toBe(false)
        expect(isBackgroundJobHandleResult(tc)).toBe(false)
      }
    })
    expect(stats.calls).toBe(0)
  })

  it('keeps every fallback the bare JSON.parse had for scalar results', () => {
    const nullResult = call({ id: 'scalar-null', name: 'task_board', status: 'success', result: 'null' })
    const numberResult = call({ id: 'scalar-num', name: 'task_board', status: 'success', result: '123' })

    expect(toolCallResultObject(nullResult)).toBeNull()
    expect(isBackgroundJobHandleResult(nullResult)).toBe(false)
    // A JSON `null` body used to throw while being read, and the summary call
    // site turned that into its "updated" line.
    expect(taskBoardToolSummary(nullResult)).toBe('任务板已更新')
    // A scalar body read fine and simply had no board fields — still no summary.
    expect(taskBoardToolSummary(numberResult)).toBeNull()
  })

  it('does not cache a bare string source (that caller mutates its own copy)', () => {
    const stats = measureJsonParse(() => {
      expect(isBackgroundJobHandleResult(HANDLE)).toBe(true)
      expect(isBackgroundJobHandleResult(HANDLE)).toBe(true)
    })
    expect(stats.calls).toBe(2)
  })
})

describe('file change results', () => {
  it('parses a write result once across repeated summaries', () => {
    const write = call({
      id: 'write-1',
      name: 'file_write',
      status: 'success',
      result: JSON.stringify({
        success: true,
        path: '/repo/src/a.ts',
        stats: { adds: 3, dels: 1 }
      })
    })

    const stats = measureJsonParse(() => {
      expect(buildFileChangeSummaries([write])[0]?.path).toBe('/repo/src/a.ts')
      expect(buildFileChangeSummaries([write])[0]?.adds).toBe(3)
    })
    expect(stats.calls).toBe(1)
  })
})
