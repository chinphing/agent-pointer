// @vitest-environment happy-dom
import { createApp, nextTick, type App } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import RenderPerfHud from './RenderPerfHud.vue'
import {
  PERF_ACTIVITY_LAYOUT_REBUILD,
  PERF_HUD_SHORTCUT_LABEL,
  PERF_HUD_SNAPSHOT_MS,
  beginPerfActivity,
  bump,
  endPerfActivity,
  record,
  recordScrollDistance,
  resetPerf,
  setGauge,
  setRenderPerfEnabled,
  setRenderPerfMemorySampler
} from '../../lib/renderPerf'
import type { MemoryGaugeReading } from '../../lib/memoryProbe'
import { CLIPBOARD_FEEDBACK_MS } from '../../lib/clipboardText'
import { MESSAGE_VIRTUAL_ROW_ESTIMATE } from '../../lib/messageVirtualization'

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

/**
 * Manual rAF harness for the tests that need to fire frames. Declared at module
 * scope (like `renderPerf.test.ts`) so TypeScript keeps the nullable type at the
 * use sites instead of narrowing it to the `null` initializer.
 */
let frameCallback: ((ts: number) => void) | null = null

function installManualFrames(): void {
  frameCallback = null
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
    frameCallback = cb as (ts: number) => void
    return 1
  })
  vi.stubGlobal('cancelAnimationFrame', () => {
    frameCallback = null
  })
}

/** Fire the pending frame callback; a no-op when none is scheduled. */
function fireFrame(ts: number): void {
  frameCallback?.(ts)
}

/** Four 250 ms ticks = one completed aggregation window, then flush the render. */
async function advanceOneSecond(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    fakeNowMs += PERF_HUD_SNAPSHOT_MS
    vi.advanceTimersByTime(PERF_HUD_SNAPSHOT_MS)
  }
  await nextTick()
}

/** The overlay's clickable block. */
function hudElement(): HTMLElement {
  const element = container?.querySelector<HTMLElement>('[data-testid="render-perf-hud"]')
  if (!element) throw new Error('HUD element is not mounted')
  return element
}

/**
 * Let a click's clipboard promise settle and the confirmation render: the handler
 * awaits the clipboard API (and possibly the fallback), so the state change lands a
 * few microtasks after the click, not on the next tick alone.
 */
async function settleCopy(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0)
  await nextTick()
}

/** Own-property override: happy-dom ships a working `navigator.clipboard`. */
function stubClipboard(clipboard: unknown): void {
  Object.defineProperty(navigator, 'clipboard', {
    value: clipboard,
    configurable: true,
    writable: true
  })
}

function stubExecCommand(execCommand: unknown): void {
  Object.defineProperty(document, 'execCommand', {
    value: execCommand,
    configurable: true,
    writable: true
  })
}

beforeEach(() => {
  // The copy confirmation rides a `setTimeout`, the snapshot tick a `setInterval`.
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout'] })
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
  Reflect.deleteProperty(navigator as unknown as Record<string, unknown>, 'clipboard')
  Reflect.deleteProperty(document as unknown as Record<string, unknown>, 'execCommand')
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('RenderPerfHud', () => {
  it('shows frames, rows and per-second counters', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    expect(hudText()).toContain(PERF_HUD_SHORTCUT_LABEL)
    expect(hudText()).toContain('fps')
    expect(hudText()).toContain('sus')
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

  it('keeps session peaks visible after the per-second counters reset', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()
    setGauge('visibleRows', 4)

    bump('render:MessageList')
    bump('render:MessageList')
    bump('render:MessageList')
    record('ms:extraScopedForWindow', 7)
    await advanceOneSecond()

    expect(hudText()).toMatch(/render:MessageList\s+3\s+pk\s+3/)
    expect(hudText()).toMatch(/ms:extraScopedForWindow\s+7\.0\s+pk\s+7\.0/)
    expect(hudText()).toMatch(/renders\/s\s+3\s+pk\s+3/)
    expect(hudText()).toMatch(/renders\/row\s+0\.75\s+pk\s+0\.75/)

    // A quiet second resets the per-second value but not the peak — this is what
    // makes a screenshot taken after a scroll still readable.
    await advanceOneSecond()
    expect(hudText()).toMatch(/render:MessageList\s+0\s+pk\s+3/)
    expect(hudText()).toMatch(/renders\/s\s+0\s+pk\s+3/)
    expect(hudText()).toMatch(/renders\/row\s+0\.00\s+pk\s+0\.75/)
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

  it('shows the accumulated scroll distance and the per-1000-px costs', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    expect(hudText()).toContain('per 1k px  n/a (nothing scrolled yet)')

    recordScrollDistance(2500)
    bump('render:MessageList')
    bump('render:MessageList')
    bump('mount:AssistantModelMessage')
    record('ms:measureElement', 10)
    record('ms:scrollPass', 2.5)
    await advanceOneSecond()

    const text = hudText()
    expect(text).toMatch(/scroll\s+2500 px/)
    // 2 renders / 2500 px, 1 mount / 2500 px, 10 ms and 2.5 ms per 2500 px.
    expect(text).toMatch(/per 1k px\s+renders 0\.80\s+mounts 0\.40/)
    expect(text).toMatch(/measure 4\.0ms\s+pass 1\.0ms/)
  })

  it('reads idle for the worst gap and the slowest second when nothing was marked', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    expect(hudText()).toMatch(/worst gap\s+0ms @ idle/)
    expect(hudText()).toMatch(/pass pk\s+0\.0ms @ idle/)
  })

  it('attributes the worst frame gap and the slowest scroll second to an operation', async () => {
    // Manual frame harness: the gap is only detected on a frame callback.
    installManualFrames()

    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    fireFrame(0)
    fireFrame(16)

    // A rebuild that opens and closes inside the 753 ms stall it causes, plus a
    // slow scroll second dominated by that same rebuild.
    fakeNowMs = 100
    beginPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    fakeNowMs = 160
    endPerfActivity(PERF_ACTIVITY_LAYOUT_REBUILD)
    record('ms:scrollPass', 86)
    fireFrame(16 + 753)

    await advanceOneSecond()

    expect(hudText()).toMatch(/worst gap\s+753ms @ layoutRebuild/)
    expect(hudText()).toMatch(/pass pk\s+86\.0ms @ layoutRebuild/)
  })

  it('shows virtualizer geometry, the row slack beside the marked blank gauge', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    setGauge('virtualTotal', 5000)
    setGauge('virtualScrollHeight', 5400)
    setGauge('virtualFirst', 4)
    setGauge('virtualLast', 19)
    setGauge('virtualMeasured', 128)
    setGauge('virtualBlank', 400)
    setGauge('virtualPullSpacer', 24)
    setGauge('virtualEstimate', MESSAGE_VIRTUAL_ROW_ESTIMATE)
    // The stripe spikes, then passes: the current value is 124, the peak 980.
    setGauge('virtualSlack', 980)
    setGauge('virtualSlack', 124)
    setGauge('virtualOverlap', 0)
    record('ms:scrollPass:read', 3.5)
    record('ms:scrollPass:write', 14.5)
    await advanceOneSecond()

    const text = hudText()
    expect(text).toMatch(/v\.total\s+5000 px/)
    expect(text).toMatch(/v\.scrollH\s+5400 px/)
    expect(text).toMatch(/v\.range\s+4-19/)
    // `v.est` sits right after the measured-row count it is the alternative to.
    expect(text).toMatch(/v\.measured\s+128\s+v\.est\s+180 px/)
    // Both halves of the pass are listed next to the whole-pass total.
    expect(text).toMatch(/ms:scrollPass\s+0\.0/)
    expect(text).toMatch(/ms:scrollPass:read\s+3\.5/)
    expect(text).toMatch(/ms:scrollPass:write\s+14\.5/)
    // `v.pull` sits on the same row, immediately after the gap it decomposes.
    expect(text).toMatch(/v\.blank\*\s+\+400 px\s+v\.pull\s+24 px/)
    // The per-row pair is the headline, each with the peak that outlives it; the
    // box-level gauge is marked as agreeing by construction, with a footnote.
    expect(text).toMatch(/v\.slack\s+124 px\s+pk\s+980 px/)
    expect(text).toMatch(/v\.overlap\s+0 px\s+pk\s+0 px/)
    expect(text).toMatch(/v\.blank agrees by construction/)
  })
})

describe('RenderPerfHud copy to clipboard', () => {
  it('copies the exact rendered text in one click', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()
    bump('render:MessageList')
    record('ms:scrollPass:write', 14.5)
    await advanceOneSecond()

    const writeText = vi.fn(async (_text: string) => {})
    stubClipboard({ writeText })

    // What is on screen at the moment of the click — every row, footnote included.
    const shown = hudText()
    expect(shown).toContain(PERF_HUD_SHORTCUT_LABEL)
    expect(shown).toContain('render:MessageList')
    expect(shown).toContain('ms:scrollPass:write')
    expect(shown).toContain('v.slack')
    expect(shown).toContain('v.blank agrees by construction')

    hudElement().click()
    await settleCopy()

    expect(writeText).toHaveBeenCalledTimes(1)
    expect(writeText).toHaveBeenCalledWith(shown)
    // The confirmation took over the title row; the payload kept the real one.
    expect(hudText()).toContain('copied')
    expect(shown).not.toContain('copied')
  })

  it('falls back to the textarea path when the primary API rejects', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    const writeText = vi.fn(async (_text: string) => {
      throw new Error('Document is not focused')
    })
    stubClipboard({ writeText })
    const execCommand = vi.fn(() => true)
    stubExecCommand(execCommand)

    const shown = hudText()
    hudElement().click()
    await settleCopy()

    expect(writeText).toHaveBeenCalledWith(shown)
    expect(execCommand).toHaveBeenCalledWith('copy')
    // The fallback succeeded, so the confirmation is the success one.
    expect(hudText()).toContain('copied')
  })

  it('shows a failure state instead of doing nothing when both paths fail', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    stubClipboard({
      writeText: async () => {
        throw new Error('denied')
      }
    })
    stubExecCommand(undefined)

    hudElement().click()
    await settleCopy()

    expect(hudText()).toContain('copy failed')
  })

  it('clears the confirmation after a couple of seconds', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()
    stubClipboard({ writeText: vi.fn(async (_text: string) => {}) })

    hudElement().click()
    await settleCopy()
    expect(hudText()).toContain('copied')

    vi.advanceTimersByTime(CLIPBOARD_FEEDBACK_MS)
    await nextTick()

    expect(hudText()).not.toContain('copied')
    // Back to the idle title row, affordance included.
    expect(hudText()).toContain(PERF_HUD_SHORTCUT_LABEL)
    expect(hudText()).toContain('click to copy')
  })

  it('never touches the clipboard on a re-render', async () => {
    setRenderPerfEnabled(true, { persist: false })
    mountHud()

    const writeText = vi.fn(async (_text: string) => {})
    stubClipboard({ writeText })

    // Several snapshot ticks, each re-rendering the whole block.
    await advanceOneSecond()
    await advanceOneSecond()

    expect(writeText).not.toHaveBeenCalled()
  })
})
