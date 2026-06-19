import { describe, expect, it } from 'vitest'
import { layoutComputerMonitors, primaryComputerMonitor } from './computerMonitorLayout'
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
