import { isTauriRuntime } from '../lib/runtime'
import { detectDesktopOs } from '../lib/desktopOs'
import { listComputerMonitors, reapplyWindowChrome, setComputerCompactChrome } from '../lib/api'
import type { ComputerMonitor } from '../types/chat'

const COMPACT_BAR_WIDTH = 400
const COMPACT_BAR_HEIGHT_SINGLE = 48
const COMPACT_BAR_HEIGHT_DOUBLE = 62
const COMPACT_MARGIN = 16
const RESTORE_MIN_WIDTH = 960
const RESTORE_MIN_HEIGHT = 640
const MACOS_CHROME_REAPPLY_DELAY_MS = 250

function delay(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms))
}

interface SavedWindowState {
  width: number
  height: number
  x: number
  y: number
  maximized: boolean
}

let saved: SavedWindowState | null = null
let compactActive = false

function appBackgroundCssColor(): string {
  const raw = getComputedStyle(document.documentElement).getPropertyValue('--background').trim()
  return raw ? `hsl(${raw})` : 'hsl(0 0% 98%)'
}

export function setCompactShellActive(active: boolean): void {
  document.documentElement.classList.toggle('computer-compact-mode', active)
}

async function setCompactWindowBackground(transparent: boolean): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const win = await loadWindow()
    await win.setBackgroundColor(transparent ? '#00000000' : appBackgroundCssColor())
  } catch (e) {
    console.warn('[computer-compact-window] setBackgroundColor failed', e)
  }
}

function compactWindowSize(twoLines: boolean): { width: number; height: number } {
  return {
    width: COMPACT_BAR_WIDTH,
    height: twoLines ? COMPACT_BAR_HEIGHT_DOUBLE : COMPACT_BAR_HEIGHT_SINGLE
  }
}

async function loadWindow() {
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  return getCurrentWindow()
}

async function monitorForWindow(
  outerX: number,
  outerY: number,
  outerW: number,
  outerH: number
): Promise<ComputerMonitor | null> {
  try {
    const monitors = await listComputerMonitors()
    if (!monitors.length) return null
    const cx = outerX + outerW / 2
    const cy = outerY + outerH / 2
    const hit = monitors.find(
      m => cx >= m.left && cx < m.left + m.width && cy >= m.top && cy < m.top + m.height
    )
    return hit ?? monitors.find(m => m.isPrimary) ?? monitors[0]
  } catch (e) {
    console.warn('[computer-compact-window] list monitors failed', e)
    return null
  }
}

async function placeCompactWindow(
  win: Awaited<ReturnType<typeof loadWindow>>,
  twoLines: boolean
): Promise<void> {
  const { PhysicalSize, PhysicalPosition } = await import('@tauri-apps/api/dpi')
  const outerSize = await win.outerSize()
  const outerPos = await win.outerPosition()
  const monitor = await monitorForWindow(outerPos.x, outerPos.y, outerSize.width, outerSize.height)

  const physicalW = compactWindowSize(twoLines).width
  const physicalH = compactWindowSize(twoLines).height

  await win.setSize(new PhysicalSize(physicalW, physicalH))

  if (monitor) {
    const x = monitor.left + monitor.width - physicalW - COMPACT_MARGIN
    const y = monitor.top + monitor.height - physicalH - COMPACT_MARGIN
    await win.setPosition(new PhysicalPosition(x, y))
  }
}

export async function shrinkComputerCompactWindow(
  twoLines: boolean,
  resizeOnly = false
): Promise<void> {
  if (!isTauriRuntime()) return
  if (!resizeOnly && compactActive) return
  try {
    const win = await loadWindow()
    const { PhysicalSize } = await import('@tauri-apps/api/dpi')

    if (!resizeOnly) {
      const maximized = await win.isMaximized()
      const outerSize = await win.outerSize()
      const outerPos = await win.outerPosition()
      saved = {
        width: outerSize.width,
        height: outerSize.height,
        x: outerPos.x,
        y: outerPos.y,
        maximized
      }

      if (maximized) await win.unmaximize()
      await win.setMinSize(new PhysicalSize(280, 48))
      await setComputerCompactChrome(true)
      await setCompactWindowBackground(true)
      compactActive = true
    }

    await placeCompactWindow(win, twoLines)
    // Re-apply after resize: some platforms reset webview bg when bounds change.
    if (!resizeOnly) {
      await setCompactWindowBackground(true)
    }
  } catch (e) {
    console.warn('[computer-compact-window] shrink failed', e)
  }
}

export async function restoreComputerCompactWindow(): Promise<void> {
  setCompactShellActive(false)
  if (!isTauriRuntime() || !compactActive) {
    compactActive = false
    saved = null
    return
  }
  try {
    const win = await loadWindow()
    const { PhysicalSize, PhysicalPosition } = await import('@tauri-apps/api/dpi')

    // Restore geometry first — resize/maximize resets macOS overlay titlebar if chrome was already reapplied.
    await win.setMinSize(new PhysicalSize(RESTORE_MIN_WIDTH, RESTORE_MIN_HEIGHT))

    if (saved) {
      await win.setSize(new PhysicalSize(saved.width, saved.height))
      await win.setPosition(new PhysicalPosition(saved.x, saved.y))
      if (saved.maximized) await win.maximize()
    }

    await setComputerCompactChrome(false)
    await setCompactWindowBackground(false)

    if (detectDesktopOs() === 'macos') {
      await delay(MACOS_CHROME_REAPPLY_DELAY_MS)
      await reapplyWindowChrome()
    }

    compactActive = false
    saved = null
  } catch (e) {
    console.warn('[computer-compact-window] restore failed', e)
    compactActive = false
    saved = null
  }
}

export function isComputerCompactWindowActive(): boolean {
  return compactActive
}
