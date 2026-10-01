import { composerAttachmentPayloadCount } from './attachmentPayloadStore'
import { EMPTY_CHAT_RETENTION, type ChatRetention } from './chatRetention'
import { canvasRgbCacheSize } from './markdownChart'
import { markdownParseCacheRetainedChars, markdownParseCacheSize } from './markdownConfig'
import { pendingDeltaCount } from './reasoningDeltaBatch'
import { installRenderPerfGaugeSampler, setGauge } from './renderPerf'
import { turnExpandConversationCount } from './turnExpandState'
import { markdownMermaidCacheSize } from '../composables/useMarkdownMermaid'
import { markdownSvgCacheSize } from '../composables/useMarkdownSvgs'

/**
 * Residency probe for the HUD's residency panel: how big every long-lived
 * frontend structure is, and how fast it is growing.
 *
 * Why countable proxies instead of bytes: the macOS WKWebView build exposes no JS
 * heap (`performance.memory` is Chromium-only), so there is no byte figure to
 * show. Entry counts and retained characters are readable and honest, and a
 * per-minute rate is what turns them into a leak signal: a transcript that grows
 * while scrolling costs characters per minute, a cache that never shrinks after
 * its bound should have evicted costs entries per minute.
 *
 * Shape, following `lib/memoryProbe.ts`:
 *
 * - **Off is free.** `installResidencyProbe()` is called from the HUD's `setup`,
 *   and `renderPerfSnapshot()` only invokes samplers while `renderPerfEnabled()`,
 *   so an installed-but-off probe reads no cache, no message and no DOM.
 * - **One tick, few seconds.** It rides the existing 4 Hz snapshot tick and reads
 *   its sources at most once per `RESIDENCY_INTERVAL_MS`. The chat totals are
 *   O(messages + tool calls) — too much for every tick, and nothing here needs
 *   sub-second resolution: memory growth is read in minutes.
 * - **Baseline at open.** The first read is the baseline (the HUD is mounted when
 *   it is toggled on, so the baseline resets with the toggle, like the peaks), and
 *   both the delta and the rate are measured from it.
 * - **`n/a` is a value.** An unavailable reading stays `null` and is not published,
 *   so the panel shows `n/a` instead of a fabricated `0`.
 * - **Sources are injected.** The cache accessors are module-level and pure, so
 *   they are called here; the chat counters need the chat store, so the caller
 *   injects them (this module never imports a store, and its tests need no Pinia).
 */

/** Source read cadence — the chat walk is O(messages + tool calls), not a tick's work. */
export const RESIDENCY_INTERVAL_MS = 3000

/** Spawn transcripts retained for the open conversation, on top of `ChatRetention`. */
export interface ResidencyChatReading extends ChatRetention {
  scopedSpawns: number
}

/** Zero reading used when no chat source is available (tests, HUD without a host). */
export const EMPTY_RESIDENCY_CHAT: ResidencyChatReading = Object.freeze({
  ...EMPTY_CHAT_RETENTION,
  scopedSpawns: 0
})

/** Everything the probe reads, injected so tests need neither Pinia nor the DOM. */
export interface ResidencyProbeSources {
  /** Retained chat text and counts for the open conversation. */
  readChat: () => ResidencyChatReading
  /** Live DOM node count, the third growth subject; `null` when unavailable. */
  readDomNodes: () => number | null
}

/**
 * One long-lived cache: its label for the panel, the gauge it is published to, and
 * the accessor that reads its container. `chars` is the second dimension of a cache
 * whose entries are not all the same size (the markdown parse cache's source text).
 */
export interface ResidencyCacheSpec {
  key: string
  label: string
  gauge: string
  read: () => number
  chars?: { gauge: string; read: () => number }
}

/**
 * The caches the panel reports, in display order. Each `gauge` is published every
 * sampled tick, so the panel renders the numbers without holding a second copy.
 */
export const RESIDENCY_CACHES: readonly ResidencyCacheSpec[] = [
  {
    key: 'markdownParse',
    label: 'md parse',
    gauge: 'residencyCacheMarkdownParse',
    read: markdownParseCacheSize,
    chars: { gauge: 'residencyCacheMarkdownParseChars', read: markdownParseCacheRetainedChars }
  },
  { key: 'svg', label: 'svg', gauge: 'residencyCacheSvg', read: markdownSvgCacheSize },
  { key: 'mermaid', label: 'mermaid', gauge: 'residencyCacheMermaid', read: markdownMermaidCacheSize },
  { key: 'canvasRgb', label: 'canvas rgb', gauge: 'residencyCacheCanvasRgb', read: canvasRgbCacheSize },
  { key: 'deltas', label: 'pending deltas', gauge: 'residencyCacheDeltas', read: pendingDeltaCount },
  {
    key: 'attachments',
    label: 'attachment payloads',
    gauge: 'residencyCacheAttachments',
    read: composerAttachmentPayloadCount
  },
  { key: 'expand', label: 'expand convs', gauge: 'residencyCacheExpand', read: turnExpandConversationCount }
]

/**
 * Gauge names for the aggregate rows. They live here rather than in `renderPerf.ts`
 * because only this module writes them and only the residency panel reads them.
 */
export const RESIDENCY_GAUGES = {
  /** Sum of every cache's entry count — one growth subject on its own. */
  cacheEntries: 'residencyCacheEntries',
  chars: 'residencyChars',
  leadChars: 'residencyLeadChars',
  scopedChars: 'residencyScopedChars',
  leadMessages: 'residencyLeadMessages',
  scopedRows: 'residencyScopedRows',
  scopedSpawns: 'residencyScopedSpawns',
  toolBodies: 'residencyToolBodies',
  asides: 'residencyAsides',
  domNodes: 'residencyDomNodes',
  elapsedMs: 'residencyElapsedMs',
  charsDelta: 'residencyCharsDelta',
  charsPerMinute: 'residencyCharsPerMinute',
  domNodesDelta: 'residencyDomNodesDelta',
  domNodesPerMinute: 'residencyDomNodesPerMinute',
  cacheEntriesDelta: 'residencyCacheEntriesDelta',
  cacheEntriesPerMinute: 'residencyCacheEntriesPerMinute'
} as const

/** One tracked value's growth since the baseline taken when the HUD was opened. */
export interface ResidencyGrowth {
  /** Value at the first read; `null` while the source is unavailable. */
  baseline: number | null
  current: number | null
  /** `current - baseline`; negative when the structure shrank. */
  delta: number | null
  /** `delta` per minute since the baseline; `null` before any time has passed. */
  perMinute: number | null
}

export interface ResidencyGrowthSet {
  chars: ResidencyGrowth
  domNodes: ResidencyGrowth
  cacheEntries: ResidencyGrowth
}

export interface ResidencySample {
  chat: ResidencyChatReading
  /** Entry count per cache key, in `RESIDENCY_CACHES` order. */
  cacheEntries: Record<string, number>
  /** Retained characters per cache that reports a second dimension. */
  cacheChars: Record<string, number>
  totalCacheEntries: number
  /** Lead transcript characters plus scoped-row characters. */
  totalChars: number
  domNodes: number | null
  growth: ResidencyGrowthSet
  /** Milliseconds since the baseline read. */
  elapsedMs: number
  /** `false` when this sample replayed the previous read (inside the throttle window). */
  read: boolean
}

export interface ResidencyProbe {
  /** Read the sources at most once per interval; between reads the last sample is replayed. */
  sample: (nowMs: number) => ResidencySample
  /** Drop the baseline so the next sample starts a fresh session. */
  reset: () => void
}

export interface ResidencyProbeHandle {
  dispose: () => void
}

/**
 * Delta and rate from a baseline. A `null` on either side stays `null` (the panel
 * shows `n/a`), and a zero — or non-positive — elapsed time yields a `null` rate
 * rather than a division by zero: the first sample after opening has no rate yet.
 */
export function residencyGrowth(
  baseline: number | null,
  current: number | null,
  elapsedMs: number
): ResidencyGrowth {
  if (baseline === null || current === null || !Number.isFinite(current)) {
    return { baseline, current, delta: null, perMinute: null }
  }
  const delta = current - baseline
  const minutes = elapsedMs / 60_000
  return { baseline, current, delta, perMinute: minutes > 0 ? delta / minutes : null }
}

export function createResidencyProbe(
  sources: ResidencyProbeSources,
  options?: { intervalMs?: number }
): ResidencyProbe {
  const intervalMs = options?.intervalMs ?? RESIDENCY_INTERVAL_MS
  /** `-Infinity` so the first `sample()` always reads (a real `0` clock is legal). */
  let lastReadAtMs = Number.NEGATIVE_INFINITY
  let baselineAtMs: number | null = null
  let baselineChars: number | null = null
  let baselineCacheEntries: number | null = null
  let baselineDomNodes: number | null = null
  let latest: ResidencySample | null = null

  function read(nowMs: number): ResidencySample {
    const chat = sources.readChat()

    const cacheEntries: Record<string, number> = {}
    const cacheChars: Record<string, number> = {}
    let totalCacheEntries = 0
    for (const cache of RESIDENCY_CACHES) {
      const entries = cache.read()
      cacheEntries[cache.key] = entries
      totalCacheEntries += entries
      if (cache.chars) cacheChars[cache.key] = cache.chars.read()
    }

    const domNodes = sources.readDomNodes()
    const totalChars = chat.leadChars + chat.scopedChars

    if (baselineAtMs === null) {
      baselineAtMs = nowMs
      baselineChars = totalChars
      baselineCacheEntries = totalCacheEntries
      baselineDomNodes = domNodes
    } else if (baselineDomNodes === null && domNodes !== null) {
      // First *available* DOM reading, mirroring `memoryProbe`'s baseline rule.
      baselineDomNodes = domNodes
    }

    const elapsedMs = nowMs - baselineAtMs
    return {
      chat,
      cacheEntries,
      cacheChars,
      totalCacheEntries,
      totalChars,
      domNodes,
      elapsedMs,
      read: true,
      growth: {
        chars: residencyGrowth(baselineChars, totalChars, elapsedMs),
        cacheEntries: residencyGrowth(baselineCacheEntries, totalCacheEntries, elapsedMs),
        domNodes: residencyGrowth(baselineDomNodes, domNodes, elapsedMs)
      }
    }
  }

  function sample(nowMs: number): ResidencySample {
    if (latest && nowMs - lastReadAtMs < intervalMs) return latest
    lastReadAtMs = nowMs
    latest = read(nowMs)
    return latest
  }

  function reset(): void {
    lastReadAtMs = Number.NEGATIVE_INFINITY
    baselineAtMs = null
    baselineChars = null
    baselineCacheEntries = null
    baselineDomNodes = null
    latest = null
  }

  return { sample, reset }
}

/**
 * Publish one sample as gauges. A `null` reading is left unpublished, so the panel
 * renders `n/a` for it instead of a stale or fabricated number.
 */
export function publishResidencyGauges(sample: ResidencySample): void {
  for (const cache of RESIDENCY_CACHES) {
    setGauge(cache.gauge, sample.cacheEntries[cache.key] ?? 0)
    if (cache.chars) setGauge(cache.chars.gauge, sample.cacheChars[cache.key] ?? 0)
  }

  setGauge(RESIDENCY_GAUGES.cacheEntries, sample.totalCacheEntries)
  setGauge(RESIDENCY_GAUGES.chars, sample.totalChars)
  setGauge(RESIDENCY_GAUGES.leadChars, sample.chat.leadChars)
  setGauge(RESIDENCY_GAUGES.scopedChars, sample.chat.scopedChars)
  setGauge(RESIDENCY_GAUGES.leadMessages, sample.chat.leadMessages)
  setGauge(RESIDENCY_GAUGES.scopedRows, sample.chat.scopedRows)
  setGauge(RESIDENCY_GAUGES.scopedSpawns, sample.chat.scopedSpawns)
  setGauge(RESIDENCY_GAUGES.toolBodies, sample.chat.toolBodies)
  setGauge(RESIDENCY_GAUGES.asides, sample.chat.asides)
  setGauge(RESIDENCY_GAUGES.elapsedMs, sample.elapsedMs)

  const growth: [ResidencyGrowth, string, string][] = [
    [sample.growth.chars, RESIDENCY_GAUGES.charsDelta, RESIDENCY_GAUGES.charsPerMinute],
    [sample.growth.domNodes, RESIDENCY_GAUGES.domNodesDelta, RESIDENCY_GAUGES.domNodesPerMinute],
    [
      sample.growth.cacheEntries,
      RESIDENCY_GAUGES.cacheEntriesDelta,
      RESIDENCY_GAUGES.cacheEntriesPerMinute
    ]
  ]
  for (const [reading, deltaGauge, rateGauge] of growth) {
    if (reading.delta !== null) setGauge(deltaGauge, reading.delta)
    if (reading.perMinute !== null) setGauge(rateGauge, reading.perMinute)
  }
  if (sample.domNodes !== null) setGauge(RESIDENCY_GAUGES.domNodes, sample.domNodes)
}

/**
 * Create a probe and drive it from the render-perf snapshot tick, so it shares the
 * HUD's cadence and its enabled flag. The returned handle unregisters it (HUD unmount).
 */
export function installResidencyProbe(sources: ResidencyProbeSources): ResidencyProbeHandle {
  const probe = createResidencyProbe(sources)
  const uninstall = installRenderPerfGaugeSampler(nowMs => {
    publishResidencyGauges(probe.sample(nowMs))
  })
  return { dispose: uninstall }
}
