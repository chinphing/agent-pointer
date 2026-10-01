import { afterEach, describe, expect, it } from 'vitest'
import {
  EMPTY_RESIDENCY_CHAT,
  RESIDENCY_CACHES,
  RESIDENCY_GAUGES,
  RESIDENCY_INTERVAL_MS,
  createResidencyProbe,
  installResidencyProbe,
  publishResidencyGauges,
  type ResidencyChatReading
} from './residencyProbe'
import {
  renderPerfSnapshot,
  resetPerf,
  setRenderPerfEnabled,
  setRenderPerfGaugeSampler
} from './renderPerf'

function makeSources(overrides: Partial<ResidencyChatReading> = {}) {
  const calls = { chat: 0, dom: 0 }
  const chat: ResidencyChatReading = { ...EMPTY_RESIDENCY_CHAT, ...overrides }
  let domNodes: number | null = 400

  return {
    calls,
    chat,
    setDomNodes(value: number | null) {
      domNodes = value
    },
    sources: {
      readChat: () => {
        calls.chat += 1
        return { ...chat }
      },
      readDomNodes: () => {
        calls.dom += 1
        return domNodes
      }
    }
  }
}

afterEach(() => {
  setRenderPerfEnabled(false, { persist: false })
  setRenderPerfGaugeSampler(null)
  resetPerf()
})

describe('createResidencyProbe growth', () => {
  it('takes the first read as the baseline, with no rate yet', () => {
    const harness = makeSources({ leadChars: 1000, scopedChars: 500 })
    const probe = createResidencyProbe(harness.sources)

    const first = probe.sample(5_000)

    expect(first.read).toBe(true)
    expect(first.elapsedMs).toBe(0)
    expect(first.totalChars).toBe(1500)
    expect(first.growth.chars).toEqual({
      baseline: 1500,
      current: 1500,
      delta: 0,
      perMinute: null // zero elapsed: no division, no invented rate
    })
  })

  it('reports the delta and the per-minute rate since the baseline', () => {
    const harness = makeSources({ leadChars: 1000 })
    const probe = createResidencyProbe(harness.sources, { intervalMs: 0 })

    probe.sample(0)
    harness.chat.leadChars = 1_240
    const after = probe.sample(60_000)

    expect(after.elapsedMs).toBe(60_000)
    expect(after.growth.chars.delta).toBe(240)
    expect(after.growth.chars.perMinute).toBe(240)
  })

  it('keeps a shrinking structure negative instead of clamping it', () => {
    const harness = makeSources({ leadChars: 2_000, scopedChars: 1_000 })
    const probe = createResidencyProbe(harness.sources, { intervalMs: 0 })

    probe.sample(0)
    harness.chat.leadChars = 1_000
    harness.chat.scopedChars = 400
    const after = probe.sample(30_000)

    expect(after.growth.chars.delta).toBe(-1600)
    expect(after.growth.chars.perMinute).toBe(-3200) // -1600 in half a minute
  })

  it('tracks the DOM nodes and the total cache entries the same way', () => {
    const harness = makeSources({ leadChars: 100 })
    const probe = createResidencyProbe(harness.sources, { intervalMs: 0 })

    const first = probe.sample(0)
    harness.setDomNodes(520)
    const after = probe.sample(120_000)

    expect(first.growth.domNodes.baseline).toBe(400)
    expect(after.growth.domNodes.delta).toBe(120)
    expect(after.growth.domNodes.perMinute).toBe(60)

    // Cache entries are read from the module accessors, so the delta is whatever
    // those containers did — here nothing changed, so it is flat.
    expect(after.growth.cacheEntries.delta).toBe(0)
    expect(after.growth.cacheEntries.perMinute).toBe(0)
  })

  it('reads once per interval and replays the sample in between', () => {
    const harness = makeSources({ leadChars: 100 })
    const probe = createResidencyProbe(harness.sources) // default interval

    const first = probe.sample(0)
    expect(harness.calls.chat).toBe(1)

    harness.chat.leadChars = 900
    const replayed = probe.sample(RESIDENCY_INTERVAL_MS - 1)

    expect(harness.calls.chat).toBe(1) // no second read
    expect(replayed).toBe(first)
    expect(replayed.totalChars).toBe(100)

    const next = probe.sample(RESIDENCY_INTERVAL_MS)
    expect(harness.calls.chat).toBe(2)
    expect(next.read).toBe(true)
    expect(next.totalChars).toBe(900)
  })

  it('re-baselines after a reset', () => {
    const harness = makeSources({ leadChars: 100 })
    const probe = createResidencyProbe(harness.sources, { intervalMs: 0 })

    probe.sample(0)
    harness.chat.leadChars = 500
    expect(probe.sample(60_000).growth.chars.delta).toBe(400)

    probe.reset()
    const afterReset = probe.sample(120_000)

    expect(afterReset.growth.chars.baseline).toBe(500)
    expect(afterReset.growth.chars.delta).toBe(0)
    expect(afterReset.elapsedMs).toBe(0)
  })
})

describe('residency cache rows', () => {
  it('reads every cache accessor and sums their entries', () => {
    const harness = makeSources()
    const probe = createResidencyProbe(harness.sources, { intervalMs: 0 })

    const sample = probe.sample(0)

    let expected = 0
    for (const cache of RESIDENCY_CACHES) {
      const entries = cache.read()
      expect(sample.cacheEntries[cache.key]).toBe(entries)
      expected += entries
      if (cache.chars) expect(sample.cacheChars[cache.key]).toBe(cache.chars.read())
    }
    expect(sample.totalCacheEntries).toBe(expected)
  })
})

describe('installResidencyProbe', () => {
  it('reads nothing while the HUD is off, and stops after dispose', () => {
    const harness = makeSources({ leadChars: 42 })
    const handle = installResidencyProbe(harness.sources)

    // Installed, but the HUD is off: the tick must not touch a source.
    renderPerfSnapshot(1_000)
    expect(harness.calls.chat).toBe(0)
    expect(harness.calls.dom).toBe(0)

    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(2_000)
    expect(harness.calls.chat).toBe(1)

    handle.dispose()
    renderPerfSnapshot(9_000)
    expect(harness.calls.chat).toBe(1)
  })

  it('publishes the panel gauges on the snapshot tick', () => {
    const harness = makeSources({ leadChars: 1_000, scopedChars: 250, scopedRows: 7, scopedSpawns: 2 })
    const handle = installResidencyProbe(harness.sources)
    setRenderPerfEnabled(true, { persist: false })

    const first = renderPerfSnapshot(1_000).gauges
    expect(first[RESIDENCY_GAUGES.chars]).toBe(1_250)
    expect(first[RESIDENCY_GAUGES.leadChars]).toBe(1_000)
    expect(first[RESIDENCY_GAUGES.scopedChars]).toBe(250)
    expect(first[RESIDENCY_GAUGES.scopedRows]).toBe(7)
    expect(first[RESIDENCY_GAUGES.scopedSpawns]).toBe(2)
    expect(first[RESIDENCY_GAUGES.domNodes]).toBe(400)
    expect(first[RESIDENCY_GAUGES.charsDelta]).toBe(0)
    // Zero elapsed on the baseline read: no rate is published, so the panel shows n/a.
    expect(first[RESIDENCY_GAUGES.charsPerMinute]).toBeUndefined()

    harness.chat.leadChars = 1_600
    harness.setDomNodes(460)
    const later = renderPerfSnapshot(1_000 + RESIDENCY_INTERVAL_MS).gauges

    // 1600 lead + 250 scoped, against the 1250 baseline.
    expect(later[RESIDENCY_GAUGES.charsDelta]).toBe(600)
    expect(later[RESIDENCY_GAUGES.domNodesDelta]).toBe(60)
    expect(later[RESIDENCY_GAUGES.charsPerMinute]).toBe(600 / (RESIDENCY_INTERVAL_MS / 60_000))

    handle.dispose()
  })

  it('leaves an unavailable reading unpublished instead of writing a zero', () => {
    const harness = makeSources()
    harness.setDomNodes(null)
    const handle = installResidencyProbe(harness.sources)
    setRenderPerfEnabled(true, { persist: false })

    const gauges = renderPerfSnapshot(1_000).gauges

    expect(gauges[RESIDENCY_GAUGES.domNodes]).toBeUndefined()
    expect(gauges[RESIDENCY_GAUGES.domNodesDelta]).toBeUndefined()
    handle.dispose()
  })
})

describe('publishResidencyGauges', () => {
  it('writes every cache row and every chat counter', () => {
    const probe = createResidencyProbe(makeSources({ leadMessages: 12 }).sources, { intervalMs: 0 })

    // `setGauge` is a no-op while the HUD is off, so the panel's own path has to
    // be exercised with the HUD on.
    setRenderPerfEnabled(true, { persist: false })
    publishResidencyGauges(probe.sample(0))

    const gauges = renderPerfSnapshot(1_000).gauges
    for (const cache of RESIDENCY_CACHES) {
      expect(gauges[cache.gauge]).toBe(cache.read())
    }
    expect(gauges[RESIDENCY_GAUGES.leadMessages]).toBe(12)
    expect(gauges[RESIDENCY_GAUGES.cacheEntries]).toBeGreaterThanOrEqual(0)
    expect(gauges[RESIDENCY_GAUGES.elapsedMs]).toBe(0)
  })
})
