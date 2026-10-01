// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  MEMORY_PROBE_INTERVAL_MS,
  advanceMetric,
  createMemoryProbe,
  formatBytes,
  formatCount,
  formatDelta,
  installMemoryProbe,
  readDomNodeCount,
  readJsHeap,
  type MemoryGaugeReading,
  type MemoryMetric,
  type MemoryProbeSources,
  type MemoryStats
} from './memoryProbe'
import {
  renderPerfSnapshot,
  resetPerf,
  setRenderPerfEnabled,
  setRenderPerfMemorySampler
} from './renderPerf'

const MB = 1024 * 1024

function sources(overrides: Partial<MemoryProbeSources> = {}): MemoryProbeSources {
  return {
    readDomNodes: () => 100,
    readHeap: () => null,
    readGauges: () => [],
    ...overrides
  }
}

function gaugeMetric(stats: MemoryStats, label: string): MemoryMetric | undefined {
  return stats.gauges.find(gauge => gauge.label === label)?.metric
}

beforeEach(() => {
  setRenderPerfEnabled(false, { persist: false })
  resetPerf()
  // No real frame loop in unit tests — only the snapshot tick matters here.
  vi.stubGlobal('requestAnimationFrame', () => 1)
  vi.stubGlobal('cancelAnimationFrame', () => {})
})

afterEach(() => {
  setRenderPerfEnabled(false, { persist: false })
  setRenderPerfMemorySampler(null)
  vi.unstubAllGlobals()
})

describe('memory formatting', () => {
  it('formats bytes in megabytes and degrades to n/a', () => {
    expect(formatBytes(null)).toBe('n/a')
    expect(formatBytes(Number.NaN)).toBe('n/a')
    expect(formatBytes(0)).toBe('0.0 MB')
    expect(formatBytes(1.5 * MB)).toBe('1.5 MB')
    expect(formatBytes(512 * MB)).toBe('512.0 MB')
  })

  it('formats counts as plain rounded integers', () => {
    expect(formatCount(null)).toBe('n/a')
    expect(formatCount(Number.NaN)).toBe('n/a')
    expect(formatCount(0)).toBe('0')
    expect(formatCount(12345)).toBe('12345')
    expect(formatCount(41.6)).toBe('42')
  })

  it('signs deltas through the same formatter as their row', () => {
    expect(formatDelta(null, formatBytes)).toBe('n/a')
    expect(formatDelta(0, formatBytes)).toBe('0.0 MB')
    expect(formatDelta(2 * MB, formatBytes)).toBe('+2.0 MB')
    expect(formatDelta(-2 * MB, formatBytes)).toBe('-2.0 MB')
    expect(formatDelta(7, formatCount)).toBe('+7')
    expect(formatDelta(-7, formatCount)).toBe('-7')
  })
})

describe('advanceMetric', () => {
  it('starts peak and baseline at the first reading', () => {
    expect(advanceMetric(undefined, 10)).toEqual({ current: 10, peak: 10, baseline: 10, delta: 0 })
  })

  it('grows the peak and measures the delta from the first reading', () => {
    let metric = advanceMetric(undefined, 10)
    metric = advanceMetric(metric, 25)
    expect(metric).toEqual({ current: 25, peak: 25, baseline: 10, delta: 15 })
    metric = advanceMetric(metric, 4)
    expect(metric).toEqual({ current: 4, peak: 25, baseline: 10, delta: -6 })
  })

  it('keeps peak and baseline across an unavailable reading', () => {
    let metric = advanceMetric(advanceMetric(undefined, 10), 25)
    metric = advanceMetric(metric, null)
    expect(metric).toEqual({ current: null, peak: 25, baseline: 10, delta: null })
    metric = advanceMetric(metric, 30)
    expect(metric).toEqual({ current: 30, peak: 30, baseline: 10, delta: 20 })
  })

  it('ignores non-finite readings instead of reporting them as 0', () => {
    expect(advanceMetric(advanceMetric(undefined, 10), Number.NaN)).toEqual({
      current: null,
      peak: 10,
      baseline: 10,
      delta: null
    })
  })
})

describe('createMemoryProbe', () => {
  it('reads the sources at most once per interval', () => {
    const readDomNodes = vi.fn(() => 100)
    const readHeap = vi.fn(() => ({ usedBytes: 1, totalBytes: 2 }))
    const readGauges = vi.fn((): MemoryGaugeReading[] => [{ label: 'lead msgs', value: 1 }])
    const probe = createMemoryProbe(sources({ readDomNodes, readHeap, readGauges }))

    probe.sample(0)
    probe.sample(250)
    probe.sample(MEMORY_PROBE_INTERVAL_MS - 1)
    expect(readDomNodes).toHaveBeenCalledTimes(1)
    expect(readHeap).toHaveBeenCalledTimes(1)
    expect(readGauges).toHaveBeenCalledTimes(1)

    probe.sample(MEMORY_PROBE_INTERVAL_MS)
    expect(readDomNodes).toHaveBeenCalledTimes(2)
    expect(readGauges).toHaveBeenCalledTimes(2)
  })

  it('replays the previous readings between source reads', () => {
    const readDomNodes = vi.fn(() => 100)
    const probe = createMemoryProbe(sources({ readDomNodes }))
    const first = probe.sample(0)
    expect(probe.sample(250)).toEqual(first)
    expect(readDomNodes).toHaveBeenCalledTimes(1)
  })

  it('tracks per-label current, peak and delta', () => {
    let leadMsgs = 10
    let spawns = 2
    const probe = createMemoryProbe(sources({
      readGauges: () => [
        { label: 'lead msgs', value: leadMsgs },
        { label: 'spawns', value: spawns }
      ]
    }))
    probe.sample(0)

    leadMsgs = 30
    spawns = 1
    const stats = probe.sample(MEMORY_PROBE_INTERVAL_MS)
    expect(gaugeMetric(stats, 'lead msgs')).toEqual({
      current: 30, peak: 30, baseline: 10, delta: 20
    })
    expect(gaugeMetric(stats, 'spawns')).toEqual({
      current: 1, peak: 2, baseline: 2, delta: -1
    })
  })

  it('reports n/a for a heap the webview does not expose', () => {
    const probe = createMemoryProbe(sources())
    const stats = probe.sample(0)
    expect(stats.heapUsed).toEqual({ current: null, peak: null, baseline: null, delta: null })
    expect(stats.heapTotalBytes).toBeNull()
    expect(stats.domNodes.current).toBe(100)
  })

  it('drops a gauge that stops being reported', () => {
    let gauges: MemoryGaugeReading[] = [{ label: 'lead msgs', value: 5 }]
    const probe = createMemoryProbe(sources({ readGauges: () => gauges }))
    probe.sample(0)
    gauges = []
    expect(probe.sample(MEMORY_PROBE_INTERVAL_MS).gauges).toEqual([])
  })

  it('reset() starts a fresh session with no peak or baseline', () => {
    let value = 10
    const probe = createMemoryProbe(sources({
      readGauges: () => [{ label: 'lead msgs', value }]
    }))
    probe.sample(0)
    value = 40
    expect(gaugeMetric(probe.sample(MEMORY_PROBE_INTERVAL_MS), 'lead msgs')?.delta).toBe(30)

    probe.reset()
    value = 12
    const stats = probe.sample(MEMORY_PROBE_INTERVAL_MS * 2)
    expect(gaugeMetric(stats, 'lead msgs')).toEqual({
      current: 12, peak: 12, baseline: 12, delta: 0
    })
  })
})

describe('installMemoryProbe', () => {
  it('stays completely idle while the HUD is off', () => {
    const readDomNodes = vi.fn(() => 100)
    const readHeap = vi.fn(() => null)
    const readGauges = vi.fn((): MemoryGaugeReading[] => [])
    const handle = installMemoryProbe(sources({ readDomNodes, readHeap, readGauges }))
    try {
      expect(renderPerfSnapshot(1000).memory).toBeNull()
      expect(renderPerfSnapshot(2000).memory).toBeNull()
      expect(readDomNodes).not.toHaveBeenCalled()
      expect(readHeap).not.toHaveBeenCalled()
      expect(readGauges).not.toHaveBeenCalled()
    } finally {
      handle.dispose()
    }
  })

  it('samples through the snapshot tick once the HUD is on', () => {
    const readDomNodes = vi.fn(() => 100)
    setRenderPerfEnabled(true, { persist: false })
    const handle = installMemoryProbe(sources({ readDomNodes }))
    try {
      expect(renderPerfSnapshot(1000).memory?.domNodes.current).toBe(100)
      expect(readDomNodes).toHaveBeenCalledTimes(1)

      // Still inside the one-second window: replayed, not re-read.
      expect(renderPerfSnapshot(1250).memory?.domNodes.current).toBe(100)
      expect(readDomNodes).toHaveBeenCalledTimes(1)

      renderPerfSnapshot(2000)
      expect(readDomNodes).toHaveBeenCalledTimes(2)
    } finally {
      handle.dispose()
    }
  })

  it('stops sampling after the HUD unmounts', () => {
    const readDomNodes = vi.fn(() => 100)
    setRenderPerfEnabled(true, { persist: false })
    const handle = installMemoryProbe(sources({ readDomNodes }))
    renderPerfSnapshot(1000)
    handle.dispose()

    expect(renderPerfSnapshot(3000).memory).toBeNull()
    expect(readDomNodes).toHaveBeenCalledTimes(1)
  })
})

describe('default readers', () => {
  it('counts DOM nodes through the live collection', () => {
    const before = readDomNodeCount()
    const divs = [0, 1, 2].map(() => document.createElement('div'))
    for (const div of divs) document.body.appendChild(div)
    expect(readDomNodeCount()).toBe(before + 3)
    for (const div of divs) div.remove()
    expect(readDomNodeCount()).toBe(before)
  })

  it('reads performance.memory when the webview exposes it', () => {
    vi.stubGlobal('performance', {
      memory: { usedJSHeapSize: 5 * MB, totalJSHeapSize: 9 * MB }
    })
    expect(readJsHeap()).toEqual({ usedBytes: 5 * MB, totalBytes: 9 * MB })
  })

  it('degrades to null without performance.memory (WKWebView)', () => {
    vi.stubGlobal('performance', {})
    expect(readJsHeap()).toBeNull()
    vi.stubGlobal('performance', { memory: { usedJSHeapSize: 'x' } })
    expect(readJsHeap()).toBeNull()
  })
})
