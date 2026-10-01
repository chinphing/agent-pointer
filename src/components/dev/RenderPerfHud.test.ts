// @vitest-environment happy-dom
import { createApp, nextTick, type App } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import RenderPerfHud from './RenderPerfHud.vue'
import {
  PERF_HUD_SHORTCUT_LABEL,
  PERF_HUD_SNAPSHOT_MS,
  bump,
  record,
  resetPerf,
  setGauge,
  setRenderPerfEnabled,
  setRenderPerfMemorySampler
} from '../../lib/renderPerf'
import type { MemoryGaugeReading } from '../../lib/memoryProbe'

let container: HTMLDivElement | null = null
let app: App | null = null
/** Manually driven clock — vitest's `toFake: ['performance']` does not advance it. */
let fakeNowMs = 0

function mountHud(props: { readGauges?: () => MemoryGaugeReading[] } = {}): void {
  container = document.createElement('div')
  document.body.appendChild(container)
  app = createApp(RenderPerfHud, props)
  app.mount(container)
}

function hudText(): string {
  return container?.textContent ?? ''
}

/** Four 250 ms ticks = one completed aggregation window, then flush the render. */
async function advanceOneSecond(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    fakeNowMs += PERF_HUD_SNAPSHOT_MS
    vi.advanceTimersByTime(PERF_HUD_SNAPSHOT_MS)
  }
  await nextTick()
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  fakeNowMs = 0
  vi.spyOn(performance, 'now').mockImplementation(() => fakeNowMs)
  setRenderPerfEnabled(false, { persist: false })
  resetPerf()
})

afterEach(() => {
  app?.unmount()
  app = null
  container?.remove()
  container = null
  setRenderPerfEnabled(false, { persist: false })
  setRenderPerfMemorySampler(null)
  vi.restoreAllMocks()
  vi.useRealTimers()
})

describe('RenderPerfHud', () => {
  it('shows frames, rows and per-second counters', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    expect(hudText()).toContain(PERF_HUD_SHORTCUT_LABEL)
    expect(hudText()).toContain('fps')
    expect(hudText()).toContain('renders/row')

    bump('render:MessageList')
    bump('render:MessageList')
    bump('mount:SubAgentContentBlock')
    bump('call:toolRunAssistantMessage')
    record('ms:extraScopedForWindow', 4.2)
    setGauge('visibleRows', 4)
    setGauge('scopedRows', 11)
    await advanceOneSecond()

    const text = hudText()
    expect(text).toMatch(/render:MessageList\s+2/)
    expect(text).toMatch(/render:AssistantModelMessage\s+0/)
    expect(text).toMatch(/mount:SubAgentContentBlock\s+1/)
    expect(text).toMatch(/call:toolRunAssistantMessage\s+1/)
    expect(text).toMatch(/ms:extraScopedForWindow\s+4\.2/)
    expect(text).toMatch(/vis rows\s+4/)
    expect(text).toMatch(/scoped\s+11/)
    // 2 renders / 4 visible rows
    expect(text).toMatch(/renders\/row\s+0\.50/)
  })

  it('shows frontend memory rows with current, peak and delta', async () => {
    setRenderPerfEnabled(true, { persist: false })
    let leadMsgs = 10
    mountHud({ readGauges: () => [{ label: 'lead msgs', value: leadMsgs }] })

    // happy-dom exposes no `performance.memory` — the heap row must say so.
    expect(hudText()).toMatch(/mem\s+heap n\/a \/ n\/a\s+pk n\/a\s+Δ n\/a/)
    expect(hudText()).toMatch(/mem\s+nodes \d+\s+pk \d+\s+Δ 0/)
    expect(hudText()).toMatch(/mem\s+lead msgs 10\s+pk 10\s+Δ 0/)

    leadMsgs = 25
    await advanceOneSecond()
    expect(hudText()).toMatch(/mem\s+lead msgs 25\s+pk 25\s+Δ \+15/)
  })

  it('clears counters and stops refreshing once the HUD is turned off', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    bump('render:MessageList')
    await advanceOneSecond()
    expect(hudText()).toMatch(/render:MessageList\s+1/)

    setRenderPerfEnabled(false, { persist: false })
    await nextTick()
    expect(hudText()).toMatch(/render:MessageList\s+0/)

    // No interval is scheduled while off, so the text freezes.
    const frozen = hudText()
    vi.advanceTimersByTime(PERF_HUD_SNAPSHOT_MS * 4)
    await nextTick()
    expect(hudText()).toBe(frozen)
  })
})
