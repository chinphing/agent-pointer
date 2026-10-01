// @vitest-environment happy-dom
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, type Ref } from 'vue'
import {
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
