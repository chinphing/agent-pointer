// @vitest-environment happy-dom
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, type Ref } from 'vue'
import { measureRenderedRowSlack } from './virtualRowSlack'
import { MESSAGE_VIRTUAL_ROW_ESTIMATE } from './messageVirtualization'
import {
  PERF_ACTIVITY_LAYOUT_REBUILD,
  PERF_ACTIVITY_LOAD_OLDER,
  PERF_ACTIVITY_MEASURE_BATCH,
  PERF_ACTIVITY_STALE_MS,
  PERF_FRAME_SUSPENSION_MS,
  PERF_GAUGE_VIRTUAL_BLANK,
  PERF_GAUGE_VIRTUAL_ESTIMATE,
  PERF_GAUGE_VIRTUAL_FIRST,
  PERF_GAUGE_VIRTUAL_LAST,
  PERF_GAUGE_VIRTUAL_MEASURED,
  PERF_GAUGE_VIRTUAL_OVERLAP,
  PERF_GAUGE_VIRTUAL_PULL_SPACER,
  PERF_GAUGE_VIRTUAL_SCROLL_HEIGHT,
  PERF_GAUGE_VIRTUAL_SLACK,
  PERF_GAUGE_VIRTUAL_TOTAL,
  PERF_GAUGE_VISIBLE_ROWS,
  PERF_HUD_SNAPSHOT_MS,
  PERF_HUD_STORAGE_KEY,
  PERF_MS_KEYS,
  PERF_MS_SCROLL_PASS,
  PERF_MS_SCROLL_PASS_READ,
  PERF_MS_SCROLL_PASS_WRITE,
  beginPerfActivity,
  bump,
  currentPerfActivity,
  endPerfActivity,
  initRenderPerf,
  installRenderPerfGaugeSampler,
  isPerfHudShortcut,
  parsePerfHudUrlFlag,
  publishVirtualGeometryGauges,
  publishVirtualRowSlackGauges,
  record,
  recordScrollDistance,
  renderPerfEnabled,
  renderPerfSnapshot,
  resetPerf,
  resolveInitialPerfHudEnabled,
  setGauge,
  setRenderPerfEnabled,
  setRenderPerfGaugeSampler,
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

/** Disposer of the geometry sampler a test installed (cleared in `afterEach`). */
let disposeGaugeSampler: (() => void) | null = null

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
  disposeGaugeSampler?.()
  disposeGaugeSampler = null
  setRenderPerfGaugeSampler(null)
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('renderPerf counters', () => {
  it('is a no-op while disabled (no counters, no frame probe)', () => {
    const framesBefore = requestFrame.mock.calls.length

    bump('render:MessageList')
    record('ms:extraScopedForWindow', 5)
    record(PERF_MS_SCROLL_PASS_READ, 3.5)
    record(PERF_MS_SCROLL_PASS_WRITE, 14.5)
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

describe('renderPerf scroll-distance normalisation', () => {
  it('accumulates |ΔscrollTop| and stays a no-op while disabled', () => {
    recordScrollDistance(120)
    recordScrollDistance(-80)
    expect(renderPerfSnapshot(1000).scrollDistancePx).toBe(0)

    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(1000)

    recordScrollDistance(120)
    recordScrollDistance(-30)
    recordScrollDistance(0)
    recordScrollDistance(Number.NaN)

    expect(renderPerfSnapshot(1500).scrollDistancePx).toBe(150)
  })

  it('derives per-1000-px costs from the session totals', () => {
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(1000)

    recordScrollDistance(2000)
    bump('render:MessageList')
    bump('render:SubAgentFrame')
    bump('mount:AssistantModelMessage')
    record('ms:measureElement', 12)
    record('ms:scrollPass', 3)
    // Another `ms:` key must not leak into the per-1000-px row.
    record('ms:extraScopedForWindow', 999)

    const snapshot = renderPerfSnapshot(2000)
    expect(snapshot.scrollDistancePx).toBe(2000)
    expect(snapshot.per1000Px).toEqual({
      renders: 1, // 2 renders / 2000 px
      mounts: 0.5, // 1 mount / 2000 px
      measureElementMs: 6, // 12 ms / 2000 px
      scrollPassMs: 1.5 // 3 ms / 2000 px
    })
  })

  it('reads zero per-1000-px costs without dividing by a zero distance', () => {
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(1000)

    bump('render:MessageList')
    record('ms:measureElement', 9)
    record('ms:scrollPass', 2)

    const snapshot = renderPerfSnapshot(2000)
    expect(snapshot.scrollDistancePx).toBe(0)
    expect(snapshot.per1000Px).toEqual({
      renders: 0,
      mounts: 0,
      measureElementMs: 0,
      scrollPassMs: 0
    })
    expect(Object.values(snapshot.per1000Px).every(Number.isFinite)).toBe(true)
  })

  it('clears the accumulated distance when the HUD is toggled', () => {
    setRenderPerfEnabled(true, { persist: false })
    recordScrollDistance(500)
    expect(renderPerfSnapshot(1000).scrollDistancePx).toBe(500)

    setRenderPerfEnabled(false, { persist: false })
    setRenderPerfEnabled(true, { persist: false })
    expect(renderPerfSnapshot(5000).scrollDistancePx).toBe(0)
  })
})

describe('renderPerf activity attribution', () => {
  it('nests activities and reports the innermost open one', () => {
    setRenderPerfEnabled(true, { persist: false })
    expect(currentPerfActivity()).toBe('')

    beginPerfActivity(PERF_ACTIVITY_LOAD_OLDER)
    expect(currentPerfActivity()).toBe(PERF_ACTIVITY_LOAD_OLDER)

    beginPerfActivity(PERF_ACTIVITY_MEASURE_BATCH)
    expect(currentPerfActivity()).toBe(PERF_ACTIVITY_MEASURE_BATCH)

    // Closing the inner one falls back to the outer, not to idle.
    endPerfActivity(PERF_ACTIVITY_MEASURE_BATCH)
    expect(currentPerfActivity()).toBe(PERF_ACTIVITY_LOAD_OLDER)

    endPerfActivity(PERF_ACTIVITY_LOAD_OLDER)
    expect(currentPerfActivity()).toBe('')
  })

  it('keeps a re-entered activity open until its last end', () => {
    setRenderPerfEnabled(true, { persist: false })

    beginPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    beginPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    endPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    expect(currentPerfActivity()).toBe(PERF_ACTIVITY_LAYOUT_REBUILD)

    endPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    expect(currentPerfActivity()).toBe('')
  })

  it('ignores an end without a begin instead of throwing or unwinding the stack', () => {
    setRenderPerfEnabled(true, { persist: false })
    beginPerfActivity(PERF_ACTIVITY_LOAD_OLDER)

    expect(() => endPerfActivity('neverBegun')).not.toThrow()
    expect(currentPerfActivity()).toBe(PERF_ACTIVITY_LOAD_OLDER)

    // A double `end` is a no-op too — it must not close the outer activity.
    endPerfActivity(PERF_ACTIVITY_LOAD_OLDER)
    expect(() => endPerfActivity(PERF_ACTIVITY_LOAD_OLDER)).not.toThrow()
    expect(currentPerfActivity()).toBe('')
  })

  it('blames the worst frame gap on the activity that ran inside it', () => {
    setRenderPerfEnabled(true, { persist: false })

    rafCallback?.(0)
    rafCallback?.(16)

    // A synchronous rebuild: it opens and closes inside the stall it causes, so
    // it is already closed by the time the next frame callback runs.
    beginPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    endPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)

    rafCallback?.(16 + 753)
    const stalled = renderPerfSnapshot(16 + 753)
    expect(stalled.frameGapMaxMs).toBe(753)
    expect(stalled.frameGapMaxActivity).toBe(PERF_ACTIVITY_LAYOUT_REBUILD)

    // A smaller gap never overwrites the attribution of the worst one.
    beginPerfActivity(PERF_ACTIVITY_MEASURE_BATCH)
    endPerfActivity(PERF_ACTIVITY_MEASURE_BATCH)
    rafCallback?.(16 + 753 + 30)
    const smaller = renderPerfSnapshot(16 + 753 + 30)
    expect(smaller.frameGapMaxMs).toBe(753)
    expect(smaller.frameGapMaxActivity).toBe(PERF_ACTIVITY_LAYOUT_REBUILD)
  })

  it('blames a gap on an activity still open when the frame fires', () => {
    setRenderPerfEnabled(true, { persist: false })

    rafCallback?.(0)
    rafCallback?.(16)

    // Async work (a page load) stays open across frames.
    beginPerfActivity(PERF_ACTIVITY_LOAD_OLDER)
    rafCallback?.(616)

    const stalled = renderPerfSnapshot(616)
    expect(stalled.frameGapMaxMs).toBe(600)
    expect(stalled.frameGapMaxActivity).toBe(PERF_ACTIVITY_LOAD_OLDER)

    endPerfActivity(PERF_ACTIVITY_LOAD_OLDER)
  })

  it('reads idle when nothing was marked during the gap', () => {
    setRenderPerfEnabled(true, { persist: false })

    rafCallback?.(0)
    rafCallback?.(16)
    rafCallback?.(916)

    const stalled = renderPerfSnapshot(916)
    expect(stalled.frameGapMaxMs).toBe(900)
    expect(stalled.frameGapMaxActivity).toBe('')
  })

  it('does not reuse a closed activity for the next gap', () => {
    setRenderPerfEnabled(true, { persist: false })

    rafCallback?.(0)
    rafCallback?.(16)

    beginPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    endPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    rafCallback?.(416)
    expect(renderPerfSnapshot(416).frameGapMaxActivity).toBe(PERF_ACTIVITY_LAYOUT_REBUILD)

    // The frame above consumed the marker, so this larger gap is unattributed.
    rafCallback?.(416 + 900)
    const idle = renderPerfSnapshot(416 + 900)
    expect(idle.frameGapMaxMs).toBe(900)
    expect(idle.frameGapMaxActivity).toBe('')
  })

  it('drops a marker whose end never came instead of blaming it forever', () => {
    let clock = 0
    vi.spyOn(performance, 'now').mockImplementation(() => clock)
    setRenderPerfEnabled(true, { persist: false })

    beginPerfActivity(PERF_ACTIVITY_LOAD_OLDER)
    expect(currentPerfActivity()).toBe(PERF_ACTIVITY_LOAD_OLDER)

    // Still inside the bound: a slow operation is not dropped.
    clock = PERF_ACTIVITY_STALE_MS
    rafCallback?.(16)
    expect(currentPerfActivity()).toBe(PERF_ACTIVITY_LOAD_OLDER)

    // Past the bound the missed `end` is assumed and the marker is dropped.
    clock = PERF_ACTIVITY_STALE_MS + 1
    rafCallback?.(32)
    expect(currentPerfActivity()).toBe('')
  })

  it('blames the worst ms:scrollPass second on the operation that dominated it', () => {
    let clock = 0
    vi.spyOn(performance, 'now').mockImplementation(() => clock)
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(0)

    // One scrolling second: 86 ms of pass time, 60 ms of it inside a rebuild and
    // 5 ms inside a measure batch — the rebuild is what that second is spent on.
    clock = 10
    beginPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    clock = 70
    endPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    clock = 80
    beginPerfActivity(PERF_ACTIVITY_MEASURE_BATCH)
    clock = 85
    endPerfActivity(PERF_ACTIVITY_MEASURE_BATCH)
    record(PERF_MS_SCROLL_PASS, 86)

    const closed = renderPerfSnapshot(1000)
    expect(closed.peakMsPerSecond[PERF_MS_SCROLL_PASS]).toBe(86)
    expect(closed.peakScrollPassActivity).toBe(PERF_ACTIVITY_LAYOUT_REBUILD)

    // A quieter second keeps both the peak and its attribution.
    const quiet = renderPerfSnapshot(2000)
    expect(quiet.peakMsPerSecond[PERF_MS_SCROLL_PASS]).toBe(86)
    expect(quiet.peakScrollPassActivity).toBe(PERF_ACTIVITY_LAYOUT_REBUILD)
  })

  it('reads idle for a slow second with nothing marked', () => {
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(0)

    record(PERF_MS_SCROLL_PASS, 86)

    const closed = renderPerfSnapshot(1000)
    expect(closed.peakMsPerSecond[PERF_MS_SCROLL_PASS]).toBe(86)
    expect(closed.peakScrollPassActivity).toBe('')
  })

  it('is a no-op while the HUD is off', () => {
    beginPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    expect(currentPerfActivity()).toBe('')
    endPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)

    rafCallback?.(0)
    rafCallback?.(900)
    record(PERF_MS_SCROLL_PASS, 86)

    const snapshot = renderPerfSnapshot(900)
    expect(snapshot.enabled).toBe(false)
    expect(snapshot.frameGapMaxMs).toBe(0)
    expect(snapshot.frameGapMaxActivity).toBe('')
    expect(snapshot.peakScrollPassActivity).toBe('')
  })

  it('clears open markers when the HUD is toggled', () => {
    setRenderPerfEnabled(true, { persist: false })
    beginPerfActivity(PERF_ACTIVITY_LOAD_OLDER)

    setRenderPerfEnabled(false, { persist: false })
    setRenderPerfEnabled(true, { persist: false })

    expect(currentPerfActivity()).toBe('')
  })
})

describe('renderPerf virtualizer geometry gauges', () => {
  it('publishes total size, scroller height, rendered range, measured rows and the blank gap', () => {
    setRenderPerfEnabled(true, { persist: false })
    publishVirtualGeometryGauges(
      { getTotalSize: () => 5000, itemSizeCache: new Map([['turn-a', 120], ['turn-b', 300]]) },
      { scrollHeight: 5400 },
      [{ index: 4 }, { index: 5 }, { index: 6 }],
      24
    )

    const gauges = renderPerfSnapshot(1000).gauges
    expect(gauges[PERF_GAUGE_VIRTUAL_TOTAL]).toBe(5000)
    expect(gauges[PERF_GAUGE_VIRTUAL_SCROLL_HEIGHT]).toBe(5400)
    expect(gauges[PERF_GAUGE_VIRTUAL_FIRST]).toBe(4)
    expect(gauges[PERF_GAUGE_VIRTUAL_LAST]).toBe(6)
    expect(gauges[PERF_GAUGE_VIRTUAL_MEASURED]).toBe(2)
    // The scroller is 400 px taller than the content: scrollable emptiness.
    expect(gauges[PERF_GAUGE_VIRTUAL_BLANK]).toBe(400)
    expect(gauges[PERF_GAUGE_VIRTUAL_PULL_SPACER]).toBe(24)
  })

  it('publishes zeros without a virtualizer or scroller instead of throwing', () => {
    setRenderPerfEnabled(true, { persist: false })

    expect(() => publishVirtualGeometryGauges(null, null, null, 0)).not.toThrow()
    expect(() => publishVirtualGeometryGauges(undefined, undefined, [], 0)).not.toThrow()

    const gauges = renderPerfSnapshot(1000).gauges
    expect(gauges[PERF_GAUGE_VIRTUAL_TOTAL]).toBe(0)
    expect(gauges[PERF_GAUGE_VIRTUAL_SCROLL_HEIGHT]).toBe(0)
    expect(gauges[PERF_GAUGE_VIRTUAL_FIRST]).toBe(-1)
    expect(gauges[PERF_GAUGE_VIRTUAL_LAST]).toBe(-1)
    expect(gauges[PERF_GAUGE_VIRTUAL_MEASURED]).toBe(0)
    expect(gauges[PERF_GAUGE_VIRTUAL_BLANK]).toBe(0)
    expect(gauges[PERF_GAUGE_VIRTUAL_PULL_SPACER]).toBe(0)
  })

  it('treats a virtualizer without the measured-size cache as zero measured rows', () => {
    setRenderPerfEnabled(true, { persist: false })

    expect(() =>
      publishVirtualGeometryGauges(
        { getTotalSize: () => 900 },
        { scrollHeight: 900 },
        [{ index: 0 }],
        0
      )
    ).not.toThrow()

    const gauges = renderPerfSnapshot(1000).gauges
    expect(gauges[PERF_GAUGE_VIRTUAL_TOTAL]).toBe(900)
    expect(gauges[PERF_GAUGE_VIRTUAL_MEASURED]).toBe(0)
    expect(gauges[PERF_GAUGE_VIRTUAL_BLANK]).toBe(0)
  })

  it('keeps the blank gap at 0 when only one side is present', () => {
    setRenderPerfEnabled(true, { persist: false })

    // Scroller but no virtualizer: 900 px of scroll height is not "blank".
    publishVirtualGeometryGauges(undefined, { scrollHeight: 900 }, [], 48)
    const scrollerOnly = renderPerfSnapshot(1000).gauges
    expect(scrollerOnly[PERF_GAUGE_VIRTUAL_BLANK]).toBe(0)
    // The spacer is a height the component renders, so it publishes regardless.
    expect(scrollerOnly[PERF_GAUGE_VIRTUAL_PULL_SPACER]).toBe(48)

    // Virtualizer but no scroller: 900 px of content is not a negative blank.
    publishVirtualGeometryGauges({ getTotalSize: () => 900 }, null, [], 0)
    const virtualizerOnly = renderPerfSnapshot(2000).gauges
    expect(virtualizerOnly[PERF_GAUGE_VIRTUAL_BLANK]).toBe(0)
    expect(virtualizerOnly[PERF_GAUGE_VIRTUAL_PULL_SPACER]).toBe(0)
  })

  it('never publishes a negative pull spacer', () => {
    setRenderPerfEnabled(true, { persist: false })

    publishVirtualGeometryGauges({ getTotalSize: () => 900 }, { scrollHeight: 900 }, [], -12)

    expect(renderPerfSnapshot(1000).gauges[PERF_GAUGE_VIRTUAL_PULL_SPACER]).toBe(0)
  })

  it('reads nothing at all while the HUD is off', () => {
    const getTotalSize = vi.fn(() => 4000)

    publishVirtualGeometryGauges(
      { getTotalSize, itemSizeCache: new Map() },
      { scrollHeight: 4000 },
      [{ index: 0 }],
      72
    )
    publishVirtualRowSlackGauges(120, 40)

    expect(getTotalSize).not.toHaveBeenCalled()
    expect(renderPerfSnapshot(1000).gauges).toEqual({})
  })

  it('exposes gauge peaks in the snapshot but nothing at all while off', () => {
    publishVirtualRowSlackGauges(980, 60)
    expect(renderPerfSnapshot(1000).peakGauges).toEqual({})

    setRenderPerfEnabled(true, { persist: false })
    publishVirtualRowSlackGauges(980, 60)
    expect(renderPerfSnapshot(2000).peakGauges).toEqual({
      [PERF_GAUGE_VIRTUAL_SLACK]: 980,
      [PERF_GAUGE_VIRTUAL_OVERLAP]: 60
    })
  })

  it('publishes the summed row slack and overlap', () => {
    setRenderPerfEnabled(true, { persist: false })

    publishVirtualRowSlackGauges(124, 0)

    const gauges = renderPerfSnapshot(1000).gauges
    expect(gauges[PERF_GAUGE_VIRTUAL_SLACK]).toBe(124)
    expect(gauges[PERF_GAUGE_VIRTUAL_OVERLAP]).toBe(0)
  })

  it('never publishes a negative slack or overlap', () => {
    setRenderPerfEnabled(true, { persist: false })

    publishVirtualRowSlackGauges(-8, Number.NaN)

    const gauges = renderPerfSnapshot(1000).gauges
    expect(gauges[PERF_GAUGE_VIRTUAL_SLACK]).toBe(0)
    expect(gauges[PERF_GAUGE_VIRTUAL_OVERLAP]).toBe(0)
  })

  it('keeps a gauge peak after the transient value has passed', () => {
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(1000)

    // The stripe spikes and is gone again before the next snapshot closes.
    publishVirtualRowSlackGauges(980, 60)
    publishVirtualRowSlackGauges(0, 0)

    const snapshot = renderPerfSnapshot(2000)
    expect(snapshot.gauges[PERF_GAUGE_VIRTUAL_SLACK]).toBe(0)
    expect(snapshot.gauges[PERF_GAUGE_VIRTUAL_OVERLAP]).toBe(0)
    expect(snapshot.peakGauges[PERF_GAUGE_VIRTUAL_SLACK]).toBe(980)
    expect(snapshot.peakGauges[PERF_GAUGE_VIRTUAL_OVERLAP]).toBe(60)
  })

  it('clears the gauge peaks when the HUD is toggled, like the other session peaks', () => {
    setRenderPerfEnabled(true, { persist: false })
    publishVirtualRowSlackGauges(980, 0)
    expect(renderPerfSnapshot(1000).peakGauges[PERF_GAUGE_VIRTUAL_SLACK]).toBe(980)

    setRenderPerfEnabled(false, { persist: false })
    setRenderPerfEnabled(true, { persist: false })

    expect(renderPerfSnapshot(5000).peakGauges).toEqual({})
  })

  it('records the two scroll-pass phases under their own keys, beside the whole pass', () => {
    setRenderPerfEnabled(true, { persist: false })
    renderPerfSnapshot(1000)

    // What `runScrollPass` records for one pass: the read half forces layout, the
    // write half applies refs / follow / prefetch / stamps / trim.
    record(PERF_MS_SCROLL_PASS_READ, 3.5)
    record(PERF_MS_SCROLL_PASS_WRITE, 14.5)
    record(PERF_MS_SCROLL_PASS, 18)

    const snapshot = renderPerfSnapshot(2000)
    expect(snapshot.msPerSecond[PERF_MS_SCROLL_PASS_READ]).toBe(3.5)
    expect(snapshot.msPerSecond[PERF_MS_SCROLL_PASS_WRITE]).toBe(14.5)
    // The whole-pass total is untouched by the split, and the two phases add up
    // to it: the read phase runs from the pass start the caller already took, and
    // the write phase from the read's end.
    expect(snapshot.msPerSecond[PERF_MS_SCROLL_PASS]).toBe(18)
    expect(
      (snapshot.msPerSecond[PERF_MS_SCROLL_PASS_READ] ?? 0)
        + (snapshot.msPerSecond[PERF_MS_SCROLL_PASS_WRITE] ?? 0)
    ).toBe(snapshot.msPerSecond[PERF_MS_SCROLL_PASS])
    expect(snapshot.peakMsPerSecond[PERF_MS_SCROLL_PASS_READ]).toBe(3.5)
    expect(snapshot.peakMsPerSecond[PERF_MS_SCROLL_PASS_WRITE]).toBe(14.5)
  })

  it('lists both phase keys for the HUD rows', () => {
    expect(PERF_MS_KEYS).toContain(PERF_MS_SCROLL_PASS)
    expect(PERF_MS_KEYS).toContain(PERF_MS_SCROLL_PASS_READ)
    expect(PERF_MS_KEYS).toContain(PERF_MS_SCROLL_PASS_WRITE)
  })

  it('publishes the assumed row height as a constant gauge', () => {
    // What the sampler publishes is the fixed estimate the virtualizer is
    // configured with (`messageVirtualizerBaseOptions`) — no measurement feeds it.
    disposeGaugeSampler = installRenderPerfGaugeSampler(() =>
      setGauge(PERF_GAUGE_VIRTUAL_ESTIMATE, MESSAGE_VIRTUAL_ROW_ESTIMATE)
    )

    // Off: the sampler never runs, so nothing is published and nothing is read.
    expect(renderPerfSnapshot(1000).gauges[PERF_GAUGE_VIRTUAL_ESTIMATE]).toBeUndefined()

    setRenderPerfEnabled(true, { persist: false })
    expect(renderPerfSnapshot(2000).gauges[PERF_GAUGE_VIRTUAL_ESTIMATE])
      .toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)
    // It stays the constant across snapshots: it cannot drift with measurements.
    expect(renderPerfSnapshot(3000).gauges[PERF_GAUGE_VIRTUAL_ESTIMATE])
      .toBe(MESSAGE_VIRTUAL_ROW_ESTIMATE)
  })

  it('runs the installed sampler once per snapshot, and only while enabled', () => {
    const virtualizer = { getTotalSize: vi.fn(() => 4000), itemSizeCache: new Map() }
    const scroller = { scrollHeight: 4120 }
    // Stands in for `MessageList`'s `noOlderPullPx` ref: read fresh every pass.
    let pullSpacerPx = 0
    // Stands in for the scroller's row elements: proves the row reads are part of
    // the same pass, and that they do not run while the HUD is off.
    const rowHeights = { 0: 300 }
    const querySelectorAll = vi.fn(() => [
      { getAttribute: () => '0', offsetHeight: rowHeights[0] }
    ])
    disposeGaugeSampler = installRenderPerfGaugeSampler(() => {
      publishVirtualGeometryGauges(virtualizer, scroller, [{ index: 0 }], pullSpacerPx)
      const rowSlack = measureRenderedRowSlack({ querySelectorAll }, [
        { index: 0, start: 0, end: 420 }
      ])
      publishVirtualRowSlackGauges(rowSlack.slack, rowSlack.overlap)
    })

    // Off: the snapshot tick must not touch the virtualizer, the scroller or the rows.
    renderPerfSnapshot(1000)
    expect(virtualizer.getTotalSize).not.toHaveBeenCalled()
    expect(querySelectorAll).not.toHaveBeenCalled()

    setRenderPerfEnabled(true, { persist: false })
    const gauges = renderPerfSnapshot(2000).gauges
    expect(virtualizer.getTotalSize).toHaveBeenCalledTimes(1)
    expect(querySelectorAll).toHaveBeenCalledTimes(1)
    expect(gauges[PERF_GAUGE_VIRTUAL_BLANK]).toBe(120)
    expect(gauges[PERF_GAUGE_VIRTUAL_PULL_SPACER]).toBe(0)
    // 420 px assumed vs a 300 px row: a 120 px blank stripe inside the window.
    expect(gauges[PERF_GAUGE_VIRTUAL_SLACK]).toBe(120)
    expect(gauges[PERF_GAUGE_VIRTUAL_OVERLAP]).toBe(0)

    // A stuck spacer lands in the very next pass, next to the gap it explains.
    pullSpacerPx = 96
    rowHeights[0] = 480
    const withSpacer = renderPerfSnapshot(3000).gauges
    expect(virtualizer.getTotalSize).toHaveBeenCalledTimes(2)
    expect(withSpacer[PERF_GAUGE_VIRTUAL_PULL_SPACER]).toBe(96)
    // The row grew past its slot: the same pass reports overlap instead of slack.
    expect(withSpacer[PERF_GAUGE_VIRTUAL_SLACK]).toBe(0)
    expect(withSpacer[PERF_GAUGE_VIRTUAL_OVERLAP]).toBe(60)
  })

  it('runs every installed sampler on the same tick', () => {
    setRenderPerfEnabled(true, { persist: false })
    const geometryCalls = vi.fn()
    const residencyCalls = vi.fn()
    const disposeGeometry = installRenderPerfGaugeSampler(nowMs => {
      geometryCalls(nowMs)
      setGauge('samplerGeometry', nowMs)
    })
    disposeGaugeSampler = installRenderPerfGaugeSampler(nowMs => {
      residencyCalls(nowMs)
      setGauge('samplerResidency', nowMs)
    })

    const gauges = renderPerfSnapshot(1000).gauges

    // One pass, both samplers, one clock.
    expect(geometryCalls).toHaveBeenCalledWith(1000)
    expect(residencyCalls).toHaveBeenCalledWith(1000)
    expect(gauges.samplerGeometry).toBe(1000)
    expect(gauges.samplerResidency).toBe(1000)

    // Disposing one leaves the other registered.
    disposeGeometry()
    expect(renderPerfSnapshot(2000).gauges.samplerResidency).toBe(2000)
    expect(geometryCalls).toHaveBeenCalledTimes(1)
  })

  it('leaves a newer sampler installed when an older one is disposed', () => {
    setRenderPerfEnabled(true, { persist: false })
    const olderCalls = vi.fn()
    const older = installRenderPerfGaugeSampler(nowMs => {
      olderCalls(nowMs)
      setGauge(PERF_GAUGE_VIRTUAL_TOTAL, 1)
    })
    disposeGaugeSampler = installRenderPerfGaugeSampler(() => setGauge(PERF_GAUGE_VIRTUAL_TOTAL, 2))

    older()

    expect(renderPerfSnapshot(1000).gauges[PERF_GAUGE_VIRTUAL_TOTAL]).toBe(2)
    // Disposed means gone, not merely outvoted by the newer writer.
    expect(olderCalls).not.toHaveBeenCalled()
  })
})
