import type { ComputerMonitor } from '../types/chat'

export interface MonitorLayoutRect {
  id: string
  monitor: ComputerMonitor
  x: number
  y: number
  width: number
  height: number
}

export interface MonitorLayout {
  rects: MonitorLayoutRect[]
  canvasWidth: number
  canvasHeight: number
}

const CANVAS_MAX_W = 520
const CANVAS_MAX_H = 300
const GAP = 12

/** Lay out monitors by desktop coordinates (like OS display arrangement). */
export function layoutComputerMonitors(monitors: ComputerMonitor[]): MonitorLayout {
  if (!monitors.length) {
    return { rects: [], canvasWidth: 0, canvasHeight: 0 }
  }

  let minLeft = monitors[0].left
  let minTop = monitors[0].top
  let maxRight = monitors[0].left + monitors[0].width
  let maxBottom = monitors[0].top + monitors[0].height

  for (const m of monitors) {
    minLeft = Math.min(minLeft, m.left)
    minTop = Math.min(minTop, m.top)
    maxRight = Math.max(maxRight, m.left + m.width)
    maxBottom = Math.max(maxBottom, m.top + m.height)
  }

  const spanW = Math.max(1, maxRight - minLeft)
  const spanH = Math.max(1, maxBottom - minTop)
  const scale = Math.min(
    (CANVAS_MAX_W - GAP * 2) / spanW,
    (CANVAS_MAX_H - GAP * 2) / spanH
  )

  const canvasWidth = Math.round(spanW * scale + GAP * 2)
  const canvasHeight = Math.round(spanH * scale + GAP * 2)

  const rects: MonitorLayoutRect[] = monitors.map(m => ({
    id: m.id,
    monitor: m,
    x: Math.round((m.left - minLeft) * scale + GAP),
    y: Math.round((m.top - minTop) * scale + GAP),
    width: Math.max(48, Math.round(m.width * scale)),
    height: Math.max(36, Math.round(m.height * scale))
  }))

  return { rects, canvasWidth, canvasHeight }
}

export function primaryComputerMonitor(monitors: ComputerMonitor[]): ComputerMonitor | undefined {
  return monitors.find(m => m.isPrimary) ?? monitors[0]
}

export function monitorShortLabel(m: ComputerMonitor): string {
  return m.isPrimary ? '主屏幕' : '扩展屏幕'
}
