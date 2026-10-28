import { describe, expect, it } from 'vitest'
import { layoutComputerMonitors, primaryComputerMonitor, resolveSendMonitorId } from './computerMonitorLayout'
import type { ComputerMonitor } from '../types/chat'

function mon(id: string, left: number, top: number, width: number, height: number, isPrimary = false): ComputerMonitor {
  return { id, left, top, width, height, isPrimary }
}

describe('layoutComputerMonitors', () => {
  it('places secondary monitor to the right of primary', () => {
    const monitors = [
      mon('p', 0, 0, 1920, 1080, true),
      mon('s', 1920, 0, 1920, 1080)
    ]
    const layout = layoutComputerMonitors(monitors)
    expect(layout.rects).toHaveLength(2)
    const primary = layout.rects.find(r => r.id === 'p')!
    const secondary = layout.rects.find(r => r.id === 's')!
    expect(secondary.x).toBeGreaterThan(primary.x)
    expect(layout.canvasWidth).toBeGreaterThan(0)
    expect(layout.canvasHeight).toBeGreaterThan(0)
  })

  it('picks primary monitor fallback', () => {
    const monitors = [mon('a', 0, 0, 800, 600), mon('b', 800, 0, 800, 600, true)]
    expect(primaryComputerMonitor(monitors)?.id).toBe('b')
  })
})

describe('resolveSendMonitorId', () => {
  const primary = mon('xcap:1', 0, 0, 1920, 1080, true)
  const extended = mon('xcap:3', 1920, 0, 1920, 1080)

  it('keeps the stored display while it is still connected', () => {
    // The regression: with auto-switch on, every send overwrote the stored id
    // with the primary display, so the agent lost the screen it had followed
    // the frontmost window to and captured the wrong one next turn.
    expect(resolveSendMonitorId('xcap:3', [primary, extended], true)).toBe('xcap:3')
  })

  it('falls back to primary when nothing is stored', () => {
    expect(resolveSendMonitorId(undefined, [primary, extended], true)).toBe('xcap:1')
  })

  it('falls back to primary when the stored display is gone', () => {
    expect(resolveSendMonitorId('xcap:9', [primary, extended], true)).toBe('xcap:1')
  })

  it('keeps the stored id when auto-switch is off', () => {
    expect(resolveSendMonitorId('xcap:3', [primary, extended], false)).toBe('xcap:3')
  })

  it('keeps a stale stored id when auto-switch is off', () => {
    // Without auto-switch the user's pick is authoritative even if that exact
    // display is not in the current list — the picker decides, not this helper.
    expect(resolveSendMonitorId('xcap:9', [primary, extended], false)).toBe('xcap:9')
  })

  it('returns undefined when there are no monitors', () => {
    expect(resolveSendMonitorId(undefined, [], true)).toBeUndefined()
  })
})
