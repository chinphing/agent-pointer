import { isTauriRuntime } from '../lib/runtime'
import { detectDesktopOs } from '../lib/desktopOs'
import {
  beginComputerCompactWindow,
  placeComputerCompactWindow,
  restoreComputerCompactWindowNative
} from '../lib/api'

const COMPACT_BAR_WIDTH = 400
const COMPACT_BAR_HEIGHT_SINGLE = 48
const COMPACT_BAR_HEIGHT_DOUBLE = 62
const COMPACT_MARGIN = 16

function appBackgroundCssColor(): string {
  const raw = getComputedStyle(document.documentElement).getPropertyValue('--background').trim()
  return raw ? `hsl(${raw})` : 'hsl(0 0% 98%)'
}

let compactActive = false

export function setCompactShellActive(active: boolean): void {
  document.documentElement.classList.toggle('computer-compact-mode', active)
}

async function setCompactWindowBackground(transparent: boolean): Promise<void> {
  if (!isTauriRuntime()) return
  if (transparent && detectDesktopOs() === 'windows') return
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    const win = getCurrentWindow()
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
    if (!resizeOnly) {
      await beginComputerCompactWindow()
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
      try {
        await restoreComputerCompactWindowNative()
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
    return
  }
  try {
    await restoreComputerCompactWindowNative()
    await setCompactWindowBackground(false)
    compactActive = false
  } catch (e) {
    console.warn('[computer-compact-window] restore failed', e)
    compactActive = false
    try {
      await setCompactWindowBackground(false)
    } catch {
      /* best-effort rollback */
    }
  }
}

export function isComputerCompactWindowActive(): boolean {
  return compactActive
}
