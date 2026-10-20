import { beforeAll, beforeEach, describe, expect, it } from 'vitest'
import type { ToolCall } from '../types/chat'
import { i18n } from '../i18n'
import {
  createJsonParseStats,
  installJsonParseProbe,
  type JsonParseStats
} from './jsonParseProbe'
import { resetToolCallArgumentsCache, toolCallArguments } from './toolCallArguments'
import {
  compactToolCallStatusLine,
  effectiveToolDisplaySummary,
  fileToolDisplayPath,
  resolveToolDisplayForCall
} from './toolCallDisplay'
import { taskBoardPatchSummaryFromArgs } from './messageTooling'
import { parseTerminalCommandFromArgs } from './terminalCommand'

beforeAll(() => {
  i18n.global.locale.value = 'zh-CN'
})

beforeEach(() => {
  resetToolCallArgumentsCache()
})

/**
 * Run `body` with the real `JSON.parse` metered (the same probe the render-perf
 * HUD installs), so a test can assert *how many* parses a render path caused and
 * whether any of them threw — the two numbers the HUD reported as pathological.
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

const WRITE_FULL = JSON.stringify({
  path: '/repo/src/lib/big.ts',
  content: `${'export const value = 1\n'.repeat(200)}`
})

/** One `ToolCallRow` render's worth of argument reads: label, path, summary, aria line. */
function renderHeaderReads(tc: ToolCall): void {
  resolveToolDisplayForCall(tc)
  effectiveToolDisplaySummary(tc)
  fileToolDisplayPath(tc, '/repo')
  compactToolCallStatusLine(tc, '/repo', { includeStatus: false })
}

describe('streaming arguments', () => {
  it('never parses a payload that is still arriving', () => {
    const streaming = call({
      id: 'stream-1',
      name: 'file_write',
      status: 'running',
      displayLabel: '写入文件',
      displaySummary: 'src/lib/big.ts',
      arguments: WRITE_FULL.slice(0, 60)
    })

    const stats = measureJsonParse(() => {
      for (let i = 0; i < 5; i += 1) renderHeaderReads(streaming)
    })

    expect(stats.calls).toBe(0)
    expect(stats.failed).toBe(0)
  })

  it('keeps the backend displaySummary in the header while arguments stream', () => {
    const streaming = call({
      id: 'stream-2',
      name: 'file_write',
      status: 'running',
      displayLabel: '写入文件',
      displaySummary: 'src/lib/big.ts',
      arguments: WRITE_FULL.slice(0, 60)
    })

    expect(fileToolDisplayPath(streaming, '/repo')).toBe('')
    expect(effectiveToolDisplaySummary(streaming)).toBe('src/lib/big.ts')
    expect(compactToolCallStatusLine(streaming, '/repo', { includeStatus: false })).toBe(
      '写入文件 · src/lib/big.ts'
    )
  })

  it('streams a long file_write with zero parses, then parses once when it closes', () => {
    const streaming = call({
      id: 'stream-3',
      name: 'file_write',
      status: 'running',
      displayLabel: '写入文件',
      displaySummary: 'src/lib/big.ts',
      arguments: ''
    })

    const whileStreaming = measureJsonParse(() => {
      // 39 slices of the payload: every one of them stops short of the closing
      // brace, which is what "still streaming" means for the parser.
      for (let i = 1; i < 40; i += 1) {
        streaming.arguments = WRITE_FULL.slice(0, Math.floor((WRITE_FULL.length * i) / 40))
        renderHeaderReads(streaming)
      }
    })
    expect(whileStreaming.calls).toBe(0)
    expect(whileStreaming.failed).toBe(0)

    const whenClosed = measureJsonParse(() => {
      streaming.arguments = WRITE_FULL
      expect(fileToolDisplayPath(streaming, '/repo')).toBe('src/lib/big.ts')
      expect(compactToolCallStatusLine(streaming, '/repo', { includeStatus: false })).toBe(
        '写入文件 · src/lib/big.ts'
      )
    })
    expect(whenClosed.calls).toBe(1)
    expect(whenClosed.failed).toBe(0)
  })

  it('keeps the raw terminal command while the payload is half written', () => {
    const partial = '{"command":"git status --short'
    expect(parseTerminalCommandFromArgs(partial)).toBe(partial)

    const parsed = '{"command":"git status","elevated":false}'
    expect(parseTerminalCommandFromArgs(parsed)).toBe('git status')
  })

  it('does not report a task_board patch while its args are incomplete', () => {
    const stats = measureJsonParse(() => {
      expect(taskBoardPatchSummaryFromArgs('{"item_id":"wi_3","status":"do')).toBeNull()
    })
    expect(stats.calls).toBe(0)
    expect(stats.failed).toBe(0)
    expect(taskBoardPatchSummaryFromArgs('{"item_id":"wi_3","status":"done"}')).toBe(
      '#wi_3 → done'
    )
  })
})

describe('settled arguments', () => {
  it('parses once and shows path + summary for a finished call', () => {
    const settled = call({
      id: 'settled-1',
      name: 'file_write',
      status: 'success',
      displayLabel: '写入文件',
      arguments: WRITE_FULL
    })

    const stats = measureJsonParse(() => {
      for (let i = 0; i < 3; i += 1) renderHeaderReads(settled)
    })

    expect(fileToolDisplayPath(settled, '/repo')).toBe('src/lib/big.ts')
    expect(compactToolCallStatusLine(settled, '/repo', { includeStatus: false })).toBe(
      '写入文件 · src/lib/big.ts'
    )
    expect(stats.calls).toBe(1)
    expect(stats.failed).toBe(0)
  })

  it('parses a failed call too (status is what settles a payload, not success)', () => {
    const failed = call({
      id: 'failed-1',
      name: 'file_edit',
      status: 'failed',
      arguments: JSON.stringify({ path: '/repo/src/a.ts', oldString: 'a', newString: 'b' })
    })
    expect(fileToolDisplayPath(failed, '/repo')).toBe('src/a.ts')
  })
})

describe('per-call argument cache', () => {
  it('parses the same object + string once', () => {
    const settled = call({
      id: 'cache-1',
      name: 'file_write',
      status: 'success',
      arguments: WRITE_FULL
    })

    const stats = measureJsonParse(() => {
      expect(toolCallArguments(settled)?.path).toBe('/repo/src/lib/big.ts')
      expect(toolCallArguments(settled)?.path).toBe('/repo/src/lib/big.ts')
      expect(toolCallArguments(settled)?.path).toBe('/repo/src/lib/big.ts')
    })
    expect(stats.calls).toBe(1)
  })

  it('re-parses only after the arguments change', () => {
    const tc = call({
      id: 'cache-2',
      name: 'file_write',
      status: 'success',
      arguments: JSON.stringify({ path: '/repo/a.ts' })
    })
    expect(toolCallArguments(tc)?.path).toBe('/repo/a.ts')

    tc.arguments = JSON.stringify({ path: '/repo/b.ts' })
    const changed = measureJsonParse(() => {
      expect(toolCallArguments(tc)?.path).toBe('/repo/b.ts')
      expect(toolCallArguments(tc)?.path).toBe('/repo/b.ts')
    })
    expect(changed.calls).toBe(1)
  })

  it('keeps two calls that share a string independent', () => {
    const raw = JSON.stringify({ path: '/repo/shared.ts' })
    const first = call({ id: 'share-1', name: 'file_read', status: 'success', arguments: raw })
    const second = call({ id: 'share-2', name: 'file_read', status: 'success', arguments: raw })

    const stats = measureJsonParse(() => {
      expect(toolCallArguments(first)?.path).toBe('/repo/shared.ts')
      expect(toolCallArguments(second)?.path).toBe('/repo/shared.ts')
    })
    expect(stats.calls).toBe(2)
  })
})

describe('incomplete payload', () => {
  it('returns null without a parse attempt and without a failed sample', () => {
    const half = '{"path":"/repo/src/a.ts","content":"line one'
    const stats = measureJsonParse(() => {
      expect(toolCallArguments(call({ id: 'half', name: 'file_write', status: 'running', arguments: half }))).toBeNull()
    })
    expect(stats.calls).toBe(0)
    expect(stats.failed).toBe(0)
  })

  it('renders empty instead of throwing or flashing a parse error', () => {
    const half = call({
      id: 'half-2',
      name: 'file_edit',
      status: 'running',
      arguments: '{"path":"/repo/src/a.ts","oldString":"x'
    })

    expect(() => renderHeaderReads(half)).not.toThrow()
    expect(fileToolDisplayPath(half, '/repo')).toBe('')
    expect(effectiveToolDisplaySummary(half)).toBe('')
    expect(compactToolCallStatusLine(half, '/repo', { includeStatus: false })).toBe('file_edit')
  })

  it('treats a closed but empty payload as "no args"', () => {
    expect(toolCallArguments(call({ id: 'empty', name: 'file_read', status: 'success', arguments: '' }))).toBeNull()
    expect(toolCallArguments(call({ id: 'blank', name: 'file_read', status: 'success', arguments: '   ' }))).toBeNull()
  })
})
