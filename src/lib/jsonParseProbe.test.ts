import { describe, expect, it, vi } from 'vitest'
import {
  JSON_PARSE_KEEP,
  JSON_PARSE_SLOW_MS,
  createJsonParseMeter,
  createJsonParseStats,
  formatJsonParseLines,
  formatParseChars,
  formatParseMs,
  formatStackFrames,
  installJsonParseProbe,
  jsonParseInputChars,
  recordJsonParseSample,
  resetJsonParseStats,
  shortenStackLocation,
  stackFrameLocation
} from './jsonParseProbe'

/** A Chrome-shaped stack whose two interesting frames are query-suffixed. */
const CHROME_STACK = [
  'Error',
  '    at Object.parse (http://localhost:5173/src/lib/jsonParseProbe.ts?t=9:214:20)',
  '    at buildToolRawArgsFromMessages (http://localhost:5173/src/lib/subAgentMessages.ts?t=9:459:28)',
  '    at SubAgentFrame.vue:496:12',
  '    at renderComponentRoot (http://localhost:5173/node_modules/.vite/deps/vue.js?v=abc:1234:5)'
].join('\n')

describe('jsonParseInputChars', () => {
  it('counts characters of a string and nothing else', () => {
    expect(jsonParseInputChars('{"a":1}')).toBe(7)
    expect(jsonParseInputChars('')).toBe(0)
    // Never coerced: a non-string would need `String()` here, which the native
    // call is the only thing allowed to do (it is what throws on `undefined`).
    expect(jsonParseInputChars(undefined)).toBe(0)
    expect(jsonParseInputChars({ toString: () => 'boom' })).toBe(0)
    expect(jsonParseInputChars(42)).toBe(0)
  })
})

describe('shortenStackLocation', () => {
  it('drops the origin, the query and the fragment', () => {
    expect(shortenStackLocation('http://localhost:5173/src/lib/subAgentMessages.ts?t=9')).toBe(
      'src/lib/subAgentMessages.ts'
    )
    expect(shortenStackLocation('file:///Users/me/app/src/lib/foo.ts')).toBe('src/lib/foo.ts')
    expect(shortenStackLocation('src/lib/foo.ts')).toBe('src/lib/foo.ts')
  })

  it('keeps the last three segments of a deep path', () => {
    expect(shortenStackLocation('http://localhost:5173/node_modules/.vite/deps/vue.js?v=1')).toBe(
      '.vite/deps/vue.js'
    )
    expect(shortenStackLocation('a/b/c/d/e.js')).toBe('c/d/e.js')
  })

  it('leaves a bare bundle name alone', () => {
    expect(shortenStackLocation('index-abc123.js')).toBe('index-abc123.js')
  })
})

describe('stackFrameLocation', () => {
  it('reads the location out of a Chrome frame', () => {
    expect(
      stackFrameLocation('    at Object.parse (http://localhost:5173/src/lib/x.ts?t=9:459:20)')
    ).toBe('src/lib/x.ts:459')
  })

  it('reads the location out of a Firefox frame', () => {
    expect(stackFrameLocation('buildToolRawArgs@http://localhost:5173/src/lib/x.ts:459:20')).toBe(
      'src/lib/x.ts:459'
    )
  })

  it('returns nothing for a frame with no location', () => {
    expect(stackFrameLocation('Error')).toBe('')
    expect(stackFrameLocation('    at Array.map (<anonymous>)')).toBe('')
  })
})

describe('formatStackFrames', () => {
  it('skips the meter frame, collapses duplicates and caps the depth', () => {
    const frames = formatStackFrames(CHROME_STACK)
    expect(frames).toEqual([
      'src/lib/subAgentMessages.ts:459',
      'SubAgentFrame.vue:496',
      '.vite/deps/vue.js:1234'
    ])
    // The probe's own wrapper frame is never the interesting one.
    expect(frames.join(' ')).not.toContain('jsonParseProbe')
  })

  it('honours maxFrames and an empty stack', () => {
    expect(formatStackFrames(CHROME_STACK, { maxFrames: 1 })).toEqual([
      'src/lib/subAgentMessages.ts:459'
    ])
    expect(formatStackFrames(CHROME_STACK, { maxFrames: 0 })).toEqual([])
    expect(formatStackFrames(undefined)).toEqual([])
    expect(formatStackFrames('')).toEqual([])
  })

  it('collapses a repeated frame that the same call site emits twice', () => {
    const repeated = [
      'Error',
      '    at parse (http://localhost:5173/src/lib/x.ts:10:1)',
      '    at parse (http://localhost:5173/src/lib/x.ts:10:1)',
      '    at y (http://localhost:5173/src/lib/y.ts:2:1)'
    ].join('\n')
    expect(formatStackFrames(repeated)).toEqual(['src/lib/x.ts:10', 'src/lib/y.ts:2'])
  })
})

describe('recordJsonParseSample', () => {
  it('accumulates calls, duration and input characters', () => {
    const stats = createJsonParseStats()
    recordJsonParseSample(stats, { ms: 1.5, chars: 100 })
    recordJsonParseSample(stats, { ms: 2.5, chars: 300 })
    expect(stats.calls).toBe(2)
    expect(stats.totalMs).toBeCloseTo(4)
    expect(stats.totalChars).toBe(400)
    expect(stats.slow).toEqual([])
    expect(stats.slowTotal).toBe(0)
    expect(stats.maxMs).toBeCloseTo(2.5)
    expect(stats.maxChars).toBe(300)
  })

  it('keeps the slowest samples first, capped, and counts the rest', () => {
    const stats = createJsonParseStats()
    for (const ms of [6, 40, 12, 9]) {
      recordJsonParseSample(stats, { ms, chars: 10, frames: [`f.ts:${ms}`] })
    }
    expect(stats.slowTotal).toBe(4)
    expect(stats.slow.map(s => s.ms)).toEqual([40, 12, 9])
    expect(stats.slow[0]?.frames).toEqual(['f.ts:40'])
    // The slowest kept sample is also the maximum.
    expect(stats.maxMs).toBe(40)
    expect(stats.maxFrames).toEqual(['f.ts:40'])
  })

  it('treats the threshold as a strict lower bound', () => {
    const stats = createJsonParseStats()
    recordJsonParseSample(stats, { ms: JSON_PARSE_SLOW_MS, chars: 1 })
    expect(stats.slowTotal).toBe(0)
    recordJsonParseSample(stats, { ms: JSON_PARSE_SLOW_MS + 0.001, chars: 1 })
    expect(stats.slowTotal).toBe(1)
  })

  it('counts failures and ignores a non-finite duration instead of poisoning the totals', () => {
    const stats = createJsonParseStats()
    recordJsonParseSample(stats, { ms: Number.NaN, chars: 5 })
    recordJsonParseSample(stats, { ms: -3, chars: 5 })
    recordJsonParseSample(stats, { ms: 1, chars: Number.NaN, failed: true })
    expect(stats.calls).toBe(3)
    expect(stats.totalMs).toBe(1)
    expect(stats.totalChars).toBe(10)
    expect(stats.failed).toBe(1)
    expect(stats.maxMs).toBe(1)
  })

  it('honours a custom threshold and cap', () => {
    const stats = createJsonParseStats()
    recordJsonParseSample(stats, { ms: 2, chars: 1 }, { slowMs: 1, keep: 1 })
    recordJsonParseSample(stats, { ms: 3, chars: 1 }, { slowMs: 1, keep: 1 })
    expect(stats.slowTotal).toBe(2)
    expect(stats.slow.map(s => s.ms)).toEqual([3])
  })
})

describe('resetJsonParseStats', () => {
  it('clears in place, keeping the object the HUD already holds', () => {
    const stats = createJsonParseStats()
    recordJsonParseSample(stats, { ms: 20, chars: 4, frames: ['a.ts:1'] })
    resetJsonParseStats(stats)
    expect(stats).toEqual(createJsonParseStats())
  })
})

describe('createJsonParseMeter', () => {
  it('measures a parse with an injected clock and keeps the slow stack', () => {
    let clock = 100
    const meter = createJsonParseMeter({
      now: () => clock,
      captureStack: () => CHROME_STACK,
      slowMs: 5
    })
    const value = meter.measure(() => {
      clock += 331
      return 'parsed'
    })
    expect(value).toBe('parsed')
    expect(meter.stats.calls).toBe(1)
    expect(meter.stats.totalMs).toBe(331)
    expect(meter.stats.slow[0]?.ms).toBe(331)
    expect(meter.stats.slow[0]?.frames[0]).toBe('src/lib/subAgentMessages.ts:459')
  })

  it('counts a fast parse without paying for a stack', () => {
    let clock = 0
    const captureStack = vi.fn(() => CHROME_STACK)
    const meter = createJsonParseMeter({ now: () => clock, captureStack })
    meter.measure(() => {
      clock += 1
      return null
    })
    expect(captureStack).not.toHaveBeenCalled()
    expect(meter.stats.slow).toEqual([])
    expect(meter.stats.calls).toBe(1)
  })

  it('rethrows and counts a parse that throws', () => {
    const meter = createJsonParseMeter({ now: () => 0 })
    expect(() =>
      meter.measure(() => {
        throw new SyntaxError('bad')
      })
    ).toThrow(SyntaxError)
    expect(meter.stats.calls).toBe(1)
    expect(meter.stats.failed).toBe(1)
  })
})

describe('installJsonParseProbe', () => {
  /** A stand-in for the global `JSON` so no test patches the real one. */
  function fakeTarget() {
    const parse = vi.fn((text: string, reviver?: (key: string, value: unknown) => unknown) => {
      const parsed = JSON.parse(text) as Record<string, unknown>
      if (typeof reviver === 'function') {
        for (const key of Object.keys(parsed)) parsed[key] = reviver(key, parsed[key])
      }
      return parsed
    })
    return { parse }
  }

  it('forwards the arguments, the reviver and the result, then restores the original', () => {
    const target = fakeTarget()
    const original = target.parse
    const stats = createJsonParseStats()
    const dispose = installJsonParseProbe({ target, stats, now: () => 0 })

    expect(target.parse).not.toBe(original)
    const reviver = (key: string, value: unknown) => (key === 'a' ? `${String(value)}!` : value)
    expect(target.parse('{"a":1}', reviver)).toEqual({ a: '1!' })
    expect(original).toHaveBeenCalledWith('{"a":1}', reviver)

    dispose()
    expect(target.parse).toBe(original)
    expect(stats.calls).toBe(1)
    expect(stats.totalChars).toBe(7)
  })

  it('rethrows the original error untouched', () => {
    const target = fakeTarget()
    const boom = new SyntaxError('Unexpected token')
    target.parse.mockImplementation(() => {
      throw boom
    })
    const stats = createJsonParseStats()
    const dispose = installJsonParseProbe({ target, stats, now: () => 0 })

    let caught: unknown
    try {
      target.parse('nope')
    } catch (error) {
      caught = error
    }
    expect(caught).toBe(boom)
    expect(stats.failed).toBe(1)
    expect(stats.slow).toEqual([])
    dispose()
  })

  it('captures a stack only above the threshold', () => {
    const target = fakeTarget()
    // Scripted clock: fast call 0→0 ms, slow call 10→341 ms.
    const readings = [0, 0, 10, 341]
    const captureStack = vi.fn(() => CHROME_STACK)
    const stats = createJsonParseStats()
    const dispose = installJsonParseProbe({
      target,
      stats,
      now: () => readings.shift() ?? 0,
      captureStack,
      slowMs: 5
    })

    target.parse('{"a":1}')
    expect(captureStack).not.toHaveBeenCalled()
    expect(stats.slow).toEqual([])

    target.parse('{"b":2}')
    expect(captureStack).toHaveBeenCalledTimes(1)
    expect(stats.slow[0]?.ms).toBe(331)
    expect(stats.slow[0]?.chars).toBe(7)
    expect(stats.slow[0]?.frames).toEqual([
      'src/lib/subAgentMessages.ts:459',
      'SubAgentFrame.vue:496',
      '.vite/deps/vue.js:1234'
    ])
    dispose()
  })

  it('cannot be stacked by a second install', () => {
    const target = fakeTarget()
    const original = target.parse
    const stats = createJsonParseStats()
    const first = installJsonParseProbe({ target, stats, now: () => 0 })
    const second = installJsonParseProbe({ target, stats, now: () => 0 })
    second()

    target.parse('{}')
    // The no-op disposer must not have restored the original under the wrapper.
    expect(stats.calls).toBe(1)
    expect(target.parse).not.toBe(original)
    first()
    expect(target.parse).toBe(original)
  })
})
describe('formatJsonParseLines', () => {
  it('always prints the summary, and says why there are no sample rows', () => {
    expect(formatJsonParseLines(createJsonParseStats())[0]).toBe(
      'parse calls 0  total 0.0ms  max 0.0ms  chars 0  slow 0'
    )
    expect(formatJsonParseLines(createJsonParseStats())[1]).toBe(
      'parse  (no JSON.parse call since the HUD opened)'
    )

    const stats = createJsonParseStats()
    recordJsonParseSample(stats, { ms: 1, chars: 20 })
    expect(formatJsonParseLines(stats)[1]).toBe('parse  (no call above 5.0ms)')
  })

  it('names the file:line of each kept slow call, slowest first', () => {
    const stats = createJsonParseStats()
    recordJsonParseSample(stats, {
      ms: 331,
      chars: 1_100_000,
      frames: ['src/lib/subAgentMessages.ts:459', 'SubAgentFrame.vue:496']
    })
    recordJsonParseSample(stats, { ms: 9, chars: 200, frames: ['src/lib/x.ts:12'] })
    recordJsonParseSample(stats, { ms: 2, chars: 10, frames: [] })

    const lines = formatJsonParseLines(stats)
    expect(lines[0]).toBe('parse calls 3  total 342.0ms  max 331.0ms  chars 1.10M  slow 2')
    expect(lines[1]).toBe('parse #1 331.0ms 1.10M ch  src/lib/subAgentMessages.ts:459 < SubAgentFrame.vue:496')
    expect(lines[2]).toBe('parse #2 9.0ms 200 ch  src/lib/x.ts:12')
    expect(lines).toHaveLength(3)
  })

  it('reports failures and never prints more rows than asked for', () => {
    const stats = createJsonParseStats()
    for (let i = 0; i < 4; i += 1) {
      recordJsonParseSample(stats, { ms: 10 + i, chars: 1, frames: [`f.ts:${i}`] }, { keep: 4 })
    }
    recordJsonParseSample(stats, { ms: 0.1, chars: 1, failed: true })
    const lines = formatJsonParseLines(stats, 2)
    expect(lines[0]).toContain('slow 4')
    expect(lines[0]).toContain('failed 1')
    expect(lines).toHaveLength(3)
    expect(lines[1]).toContain('parse #1')
    expect(lines[2]).toContain('parse #2')
  })
})

describe('parse formatters', () => {
  it('keeps character counts and durations short', () => {
    expect(formatParseChars(0)).toBe('0')
    expect(formatParseChars(9999)).toBe('9999')
    expect(formatParseChars(12_345)).toBe('12.3k')
    expect(formatParseChars(2_500_000)).toBe('2.50M')
    expect(formatParseMs(0)).toBe('0.0ms')
    expect(formatParseMs(331.04)).toBe('331.0ms')
  })

  it('documents its defaults', () => {
    expect(JSON_PARSE_SLOW_MS).toBe(5)
    expect(JSON_PARSE_KEEP).toBeGreaterThanOrEqual(3)
  })
})
