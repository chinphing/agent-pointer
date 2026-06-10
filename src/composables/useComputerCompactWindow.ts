import { isTauriRuntime } from '../lib/runtime'
import { detectDesktopOs } from '../lib/desktopOs'
import { placeComputerCompactWindow, reapplyWindowChrome, setComputerCompactChrome } from '../lib/api'

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

async function placeCompactWindow(twoLines: boolean): Promise<void> {
  const { width, height } = compactWindowSize(twoLines)
  await placeComputerCompactWindow(width, height, COMPACT_MARGIN)
}

export async function shrinkComputerCompactWindow(
  twoLines: boolean,
  resizeOnly = false
): Promise<boolean> {
  if (!isTauriRuntime()) return false
  if (!resizeOnly && compactActive) return true
  try {
    const win = await loadWindow()
    const { LogicalSize } = await import('@tauri-apps/api/dpi')

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
      await win.setMinSize(new LogicalSize(280, 48))
      await setComputerCompactChrome(true)
      await setCompactWindowBackground(true)
      compactActive = true
    }

    await placeCompactWindow(twoLines)
    if (!resizeOnly) {
      await setCompactWindowBackground(true)
    }
    return true
  } catch (e) {
    console.warn('[computer-compact-window] shrink failed', e)
    if (!resizeOnly) {
      compactActive = false
      saved = null
      try {
        await setComputerCompactChrome(false)
        await setCompactWindowBackground(false)
      } catch {
        /* best-effort rollback */
      }
    }
    return false
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
    const { LogicalSize, PhysicalSize, PhysicalPosition } = await import('@tauri-apps/api/dpi')

    await win.setMinSize(new LogicalSize(RESTORE_MIN_WIDTH, RESTORE_MIN_HEIGHT))

    if (saved) {
      await win.setSize(new PhysicalSize(saved.width, saved.height))
      await win.setPosition(new PhysicalPosition(saved.x, saved.y))
      if (saved.maximized) await win.maximize()
    }

    await setComputerCompactChrome(false)
    await setCompactWindowBackground(false)

    const os = detectDesktopOs()
    if (os === 'macos') {
      await delay(MACOS_CHROME_REAPPLY_DELAY_MS)
    }
    if (os === 'macos' || os === 'windows' || os === 'linux') {
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
