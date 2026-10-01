// @vitest-environment happy-dom
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, type Ref } from 'vue'
import {
  PERF_FRAME_SUSPENSION_MS,
  PERF_GAUGE_VISIBLE_ROWS,
  PERF_HUD_SNAPSHOT_MS,
  PERF_HUD_STORAGE_KEY,
  bump,
  initRenderPerf,
  isPerfHudShortcut,
  parsePerfHudUrlFlag,
  record,
  renderPerfEnabled,
  renderPerfSnapshot,
  resetPerf,
  resolveInitialPerfHudEnabled,
  setGauge,
  setRenderPerfEnabled,
  toggleRenderPerf,
  useRenderPerfEnabled,
  useRenderPerfSnapshot,
  type RenderPerfSnapshot
} from './renderPerf'

/** Manual rAF harness so frame sampling is deterministic (no real clock). */
let rafCallback: ((ts: number) => void) | null = null
const requestFrame = vi.fn((cb: FrameRequestCallback) => {
  rafCallback = cb as (ts: number) => void
  return 1
})
const cancelFrame = vi.fn(() => {
  rafCallback = null
})

beforeAll(() => {
  localStorage.clear()
  initRenderPerf()
})

beforeEach(() => {
  localStorage.clear()
  setRenderPerfEnabled(false, { persist: false })
  resetPerf()
  rafCallback = null
  requestFrame.mockClear()
  cancelFrame.mockClear()
  vi.stubGlobal('requestAnimationFrame', requestFrame)
  vi.stubGlobal('cancelAnimationFrame', cancelFrame)
})

afterEach(() => {
  setRenderPerfEnabled(false, { persist: false })
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('renderPerf counters', () => {
  it('is a no-op while disabled (no counters, no frame probe)', () => {
    const framesBefore = requestFrame.mock.calls.length

    bump('render:MessageList')
    record('ms:extraScopedForWindow', 5)
    setGauge(PERF_GAUGE_VISIBLE_ROWS, 9)

    const snapshot = renderPerfSnapshot(1000)
    expect(renderPerfEnabled()).toBe(false)
    expect(snapshot.enabled).toBe(false)
    expect(snapshot.totalRendersPerSecond).toBe(0)
    expect(snapshot.renders).toEqual({})
    expect(snapshot.mounts).toEqual({})
    expect(snapshot.msPerSecond).toEqual({})
    expect(snapshot.gauges).toEqual({})
    expect(snapshot.rendersPerVisibleRow).toBe(0)
    expect(snapshot.peakRenders).toEqual({})
    expect(snapshot.peakMounts).toEqual({})
    expect(snapshot.peakCalls).toEqual({})
    expect(snapshot.peakMsPerSecond).toEqual({})
    expect(snapshot.totalRendersPerSecondPeak).toBe(0)
    expect(snapshot.rendersPerVisibleRowPeak).toBe(0)
    expect(snapshot.frameSuspensions).toBe(0)
    expect(requestFrame.mock.calls.length).toBe(framesBefore)
  })

  it('aggregates render and mount counts per completed second', () => {
    setRenderPerfEnabled(true, { persist: false })
    expect(requestFrame).toHaveBeenCalledTimes(1)

    bump('render:MessageList')
    // First snapshot only establishes the window — nothing has closed yet.
    expect(renderPerfSnapshot(1000).renders).toEqual({})

    bump('render:MessageList')
    bump('render:MessageList')
    bump('mount:SubAgentContentBlock')
    // 999 ms in: the window is still open, so the closed values stay empty.
    const open = renderPerfSnapshot(1999)
    expect(open.totalRendersPerSecond).toBe(0)
    expect(open.mounts).toEqual({})

    // 1000 ms in: the window closes and reports.
    const closed = renderPerfSnapshot(2000)
    expect(closed.renders['render:MessageList']).toBe(3)
    expect(closed.mounts['mount:SubAgentContentBlock']).toBe(1)
    expect(closed.totalRendersPerSecond).toBe(3)

    // Counters reset for the next second.
    bump('render:SubAgentFrame')
    const next = renderPerfSnapshot(3000)
    expect(next.renders['render:MessageList']).toBeUndefined()
    expect(next.renders['render:SubAgentFrame']).toBe(1)
    expect(next.totalRendersPerSecond).toBe(1)
  })

  it('accumulates milliseconds and ignores invalid durations', () => {
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(1000)

    record('ms:extraScopedForWindow', 2.5)
    record('ms:extraScopedForWindow', 1.5)
    record('ms:measureElement', Number.NaN)
    record('ms:measureElement', -3)

    const snapshot = renderPerfSnapshot(2000)
    expect(snapshot.msPerSecond['ms:extraScopedForWindow']).toBeCloseTo(4)
    expect(snapshot.msPerSecond['ms:measureElement']).toBeUndefined()
  })

  it('tracks a session peak per counter key, surviving the per-second reset', () => {
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(1000)

    bump('render:MessageList')
    bump('render:MessageList')
    bump('render:MessageList')
    bump('mount:SubAgentFrame')
    bump('call:toolRunAssistantMessage')
    record('ms:extraScopedForWindow', 4)

    const closed = renderPerfSnapshot(2000)
    expect(closed.renders['render:MessageList']).toBe(3)
    expect(closed.peakRenders['render:MessageList']).toBe(3)
    expect(closed.peakMounts['mount:SubAgentFrame']).toBe(1)
    expect(closed.peakCalls['call:toolRunAssistantMessage']).toBe(1)
    expect(closed.peakMsPerSecond['ms:extraScopedForWindow']).toBe(4)

    // A quiet second drops the per-second value but keeps the peak.
    const quiet = renderPerfSnapshot(3000)
    expect(quiet.renders['render:MessageList']).toBeUndefined()
    expect(quiet.peakRenders['render:MessageList']).toBe(3)
    expect(quiet.peakMsPerSecond['ms:extraScopedForWindow']).toBe(4)

    // A busier second raises the peak.
    for (let i = 0; i < 5; i++) bump('render:MessageList')
    const busy = renderPerfSnapshot(4000)
    expect(busy.renders['render:MessageList']).toBe(5)
    expect(busy.peakRenders['render:MessageList']).toBe(5)
  })

  it('tracks renders/s and renders/row peaks without letting 0 rows reset them', () => {
    setRenderPerfEnabled(true, { persist: false })
    setGauge(PERF_GAUGE_VISIBLE_ROWS, 4)
    renderPerfSnapshot(1000)

    for (let i = 0; i < 8; i++) bump('render:SubAgentFrame')
    const busy = renderPerfSnapshot(2000)
    expect(busy.totalRendersPerSecond).toBe(8)
    expect(busy.rendersPerVisibleRow).toBeCloseTo(2)
    expect(busy.totalRendersPerSecondPeak).toBe(8)
    expect(busy.rendersPerVisibleRowPeak).toBeCloseTo(2)

    bump('render:SubAgentFrame')
    bump('render:SubAgentFrame')
    const quieter = renderPerfSnapshot(3000)
    expect(quieter.totalRendersPerSecond).toBe(2)
    expect(quieter.totalRendersPerSecondPeak).toBe(8)
    expect(quieter.rendersPerVisibleRowPeak).toBeCloseTo(2)

    // No visible rows: the ratio reads 0, and that must not become the peak.
    setGauge(PERF_GAUGE_VISIBLE_ROWS, 0)
    for (let i = 0; i < 20; i++) bump('render:SubAgentFrame')
    const noRows = renderPerfSnapshot(4000)
    expect(noRows.totalRendersPerSecond).toBe(20)
    expect(noRows.totalRendersPerSecondPeak).toBe(20)
    expect(noRows.rendersPerVisibleRow).toBe(0)
    expect(noRows.rendersPerVisibleRowPeak).toBeCloseTo(2)
  })

  it('clears the peaks when the HUD is toggled, like the memory deltas', () => {
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(1000)
    bump('render:MessageList')
    expect(renderPerfSnapshot(2000).peakRenders['render:MessageList']).toBe(1)

    setRenderPerfEnabled(false, { persist: false })
    setRenderPerfEnabled(true, { persist: false })
    const fresh = renderPerfSnapshot(5000)
    expect(fresh.peakRenders).toEqual({})
    expect(fresh.peakMounts).toEqual({})
    expect(fresh.peakCalls).toEqual({})
    expect(fresh.peakMsPerSecond).toEqual({})
    expect(fresh.totalRendersPerSecondPeak).toBe(0)
    expect(fresh.rendersPerVisibleRowPeak).toBe(0)
  })

  it('derives renders-per-visible-row from the latest gauges', () => {
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(1000)

    setGauge(PERF_GAUGE_VISIBLE_ROWS, 4)
    setGauge('scopedRows', 12)
    bump('render:SubAgentFrame')
    bump('render:SubAgentFrame')

    const snapshot = renderPerfSnapshot(2000)
    expect(snapshot.gauges[PERF_GAUGE_VISIBLE_ROWS]).toBe(4)
    expect(snapshot.gauges.scopedRows).toBe(12)
    expect(snapshot.rendersPerVisibleRow).toBeCloseTo(0.5)

    setGauge(PERF_GAUGE_VISIBLE_ROWS, 0)
    expect(renderPerfSnapshot(2000).rendersPerVisibleRow).toBe(0)
  })

  it('reports trailing-second fps and frame gaps, tracking the session maximum', () => {
    setRenderPerfEnabled(true, { persist: false })
    expect(requestFrame).toHaveBeenCalledTimes(1)

    rafCallback?.(0)
    rafCallback?.(16)
    rafCallback?.(32)
    rafCallback?.(1000)

    const snapshot = renderPerfSnapshot(1000)
    expect(snapshot.fps).toBe(4)
    expect(snapshot.frameGapMs).toBe(968)
    expect(snapshot.frameGapMaxMs).toBe(968)

    // Frames older than the trailing second drop out of the window…
    const later = renderPerfSnapshot(1900)
    expect(later.fps).toBe(1)
    expect(later.frameGapMs).toBe(0)
    // …but the session maximum is kept.
    expect(later.frameGapMaxMs).toBe(968)
  })

  it('counts a suspended frame loop separately instead of as a frame gap', () => {
    setRenderPerfEnabled(true, { persist: false })

    rafCallback?.(0)
    rafCallback?.(16)
    rafCallback?.(32)
    expect(renderPerfSnapshot(32).frameGapMaxMs).toBe(16)

    // Hidden page / sleeping display: the loop resumes 77 s later. That is a
    // suspension, not a frame the app dropped.
    rafCallback?.(32 + 77_000)
    rafCallback?.(32 + 77_800)
    const resumed = renderPerfSnapshot(32 + 77_800)
    expect(resumed.frameSuspensions).toBe(1)
    // The 77 s suspension is not the maximum; the 800 ms gap after resuming is.
    expect(resumed.frameGapMaxMs).toBe(800)
    expect(resumed.frameGapMs).toBe(800)

    // A real stall below the threshold still raises the maximum.
    rafCallback?.(32 + 78_700)
    expect(renderPerfSnapshot(32 + 78_700).frameGapMaxMs).toBe(900)
  })

  it('treats the suspension threshold as a strict lower bound', () => {
    setRenderPerfEnabled(true, { persist: false })

    rafCallback?.(0)
    rafCallback?.(PERF_FRAME_SUSPENSION_MS)
    const atThreshold = renderPerfSnapshot(PERF_FRAME_SUSPENSION_MS)
    expect(atThreshold.frameSuspensions).toBe(0)
    expect(atThreshold.frameGapMaxMs).toBe(PERF_FRAME_SUSPENSION_MS)

    rafCallback?.(PERF_FRAME_SUSPENSION_MS * 2 + 1)
    const above = renderPerfSnapshot(PERF_FRAME_SUSPENSION_MS * 2 + 1)
    expect(above.frameSuspensions).toBe(1)
    expect(above.frameGapMaxMs).toBe(PERF_FRAME_SUSPENSION_MS)
  })

  it('stops the frame probe and clears samples when disabled', () => {
    setRenderPerfEnabled(true, { persist: false })
    rafCallback?.(10)
    rafCallback?.(20)
    setRenderPerfEnabled(false, { persist: false })

    expect(cancelFrame).toHaveBeenCalledTimes(1)
    const snapshot = renderPerfSnapshot(1000)
    expect(snapshot.enabled).toBe(false)
    expect(snapshot.fps).toBe(0)
    expect(snapshot.frameGapMaxMs).toBe(0)
  })
})

describe('renderPerf toggle sources', () => {
  it('parses the ?perfHud URL flag', () => {
    expect(parsePerfHudUrlFlag('')).toBeNull()
    expect(parsePerfHudUrlFlag('?other=1')).toBeNull()
    expect(parsePerfHudUrlFlag('?perfHud=1')).toBe(true)
    expect(parsePerfHudUrlFlag('perfHud=1')).toBe(true)
    expect(parsePerfHudUrlFlag('?perfHud=0')).toBe(false)
    expect(parsePerfHudUrlFlag('?perfHud=false')).toBe(false)
    expect(parsePerfHudUrlFlag('?a=1&perfHud=off')).toBe(false)
  })

  it('lets the URL flag override the stored preference', () => {
    expect(resolveInitialPerfHudEnabled({ stored: false, urlFlag: null })).toBe(false)
    expect(resolveInitialPerfHudEnabled({ stored: true, urlFlag: null })).toBe(true)
    expect(resolveInitialPerfHudEnabled({ stored: false, urlFlag: true })).toBe(true)
    expect(resolveInitialPerfHudEnabled({ stored: true, urlFlag: false })).toBe(false)
  })

  it('persists the toggle in localStorage', () => {
    setRenderPerfEnabled(true)
    expect(localStorage.getItem(PERF_HUD_STORAGE_KEY)).toBe('1')
    setRenderPerfEnabled(false)
    expect(localStorage.getItem(PERF_HUD_STORAGE_KEY)).toBeNull()
    setRenderPerfEnabled(true)
    expect(toggleRenderPerf()).toBe(false)
    expect(localStorage.getItem(PERF_HUD_STORAGE_KEY)).toBeNull()
  })

  it('matches only the Cmd/Ctrl+Shift+Alt+P chord', () => {
    const base = { key: 'p', code: 'KeyP', altKey: true, shiftKey: true, metaKey: true, ctrlKey: false }
    expect(isPerfHudShortcut(base)).toBe(true)
    expect(isPerfHudShortcut({ ...base, metaKey: false, ctrlKey: true })).toBe(true)
    expect(isPerfHudShortcut({ ...base, metaKey: false })).toBe(false)
    expect(isPerfHudShortcut({ ...base, altKey: false })).toBe(false)
    expect(isPerfHudShortcut({ ...base, shiftKey: false })).toBe(false)
    expect(isPerfHudShortcut({ ...base, key: 'q', code: 'KeyQ' })).toBe(false)
    // macOS Option+P rewrites `key`; the physical code still matches.
    expect(isPerfHudShortcut({ ...base, key: 'π' })).toBe(true)
  })

  it('toggles from a real keydown and from the console escape hatch', () => {
    window.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'p',
        code: 'KeyP',
        altKey: true,
        shiftKey: true,
        metaKey: true,
        bubbles: true
      })
    )
    expect(renderPerfEnabled()).toBe(true)
    expect(localStorage.getItem(PERF_HUD_STORAGE_KEY)).toBe('1')

    window.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'p',
        code: 'KeyP',
        altKey: true,
        shiftKey: true,
        ctrlKey: true,
        bubbles: true
      })
    )
    expect(renderPerfEnabled()).toBe(false)

    expect(window.__pointerPerfHud?.(true)).toBe(true)
    expect(renderPerfEnabled()).toBe(true)
    expect(window.__pointerPerfHud?.(false)).toBe(false)
    expect(renderPerfEnabled()).toBe(false)
  })
})

describe('renderPerf Vue bindings', () => {
  it('mirrors the enabled flag and refreshes snapshots at 4 Hz', () => {
    vi.useFakeTimers()
    const scope = effectScope()

    const enabledRef = scope.run(() => useRenderPerfEnabled()) as Ref<boolean>
    expect(enabledRef.value).toBe(false)

    setRenderPerfEnabled(true, { persist: false })
    expect(enabledRef.value).toBe(true)

    const snapshot = scope.run(() => useRenderPerfSnapshot()) as Ref<RenderPerfSnapshot>
    expect(snapshot.value.enabled).toBe(true)
    const first = snapshot.value

    vi.advanceTimersByTime(PERF_HUD_SNAPSHOT_MS)
    expect(snapshot.value).not.toBe(first)

    setRenderPerfEnabled(false, { persist: false })
    expect(enabledRef.value).toBe(false)
    expect(snapshot.value.enabled).toBe(false)

    const afterDisable = snapshot.value
    vi.advanceTimersByTime(PERF_HUD_SNAPSHOT_MS * 4)
    expect(snapshot.value).toBe(afterDisable)

    scope.stop()
  })
})
