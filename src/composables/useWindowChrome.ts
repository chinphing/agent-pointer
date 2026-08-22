import { computed, onMounted, onUnmounted, ref } from 'vue'
import { detectDesktopOs, macTrafficLightInsetActive, type DesktopOs } from '../lib/desktopOs'
import { reapplyWindowChrome } from '../lib/api'
import { isTauriRuntime } from '../lib/runtime'

const MACOS_CHROME_REPAIR_DEBOUNCE_MS = 400
/** Fullscreen can lag the resize event; re-query after this delay. */
const FULLSCREEN_STATE_LAG_MS = 80

/** Last known window state so a newly mounted view (settings vs chat) paints the correct inset. */
let sharedMaximized = false
let sharedFullscreen = false

let dragWindow: import('@tauri-apps/api/window').Window | null = null
if (isTauriRuntime()) {
  void import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
    dragWindow = getCurrentWindow()
  })
}

export type { DesktopOs }

/** Native traffic lights (macOS overlay); custom buttons on Windows/Linux. */
export function useWindowChrome() {
  const enabled = isTauriRuntime()
  const os = ref<DesktopOs>(enabled ? detectDesktopOs() : 'unknown')
  const maximized = ref(sharedMaximized)
  const fullscreen = ref(sharedFullscreen)

  const showCustomControls = ref(os.value === 'windows' || os.value === 'linux')
  const macTrafficLightPadding = computed(() => macTrafficLightInsetActive(os.value, fullscreen.value))

  let unlistenResize: (() => void) | undefined
  let macosChromeRepairTimer: ReturnType<typeof setTimeout> | undefined
  let fullscreenLagTimer: ReturnType<typeof setTimeout> | undefined

  function scheduleMacosChromeRepair() {
    if (os.value !== 'macos') return
    if (macosChromeRepairTimer) clearTimeout(macosChromeRepairTimer)
    macosChromeRepairTimer = setTimeout(() => {
      macosChromeRepairTimer = undefined
      void reapplyWindowChrome().catch(e => {
        console.warn('[window-chrome] macOS chrome repair failed', e)
      })
    }, MACOS_CHROME_REPAIR_DEBOUNCE_MS)
  }

  async function refreshWindowState(win: import('@tauri-apps/api/window').Window) {
    try {
      const [isMax, isFs] = await Promise.all([win.isMaximized(), win.isFullscreen()])
      sharedMaximized = isMax
      sharedFullscreen = isFs
      maximized.value = isMax
      fullscreen.value = isFs
    } catch (e) {
      console.warn('[window-chrome] window state refresh failed', e)
    }
  }

  function scheduleFullscreenLagRefresh(win: import('@tauri-apps/api/window').Window) {
    if (fullscreenLagTimer) clearTimeout(fullscreenLagTimer)
    fullscreenLagTimer = setTimeout(() => {
      fullscreenLagTimer = undefined
      void refreshWindowState(win)
    }, FULLSCREEN_STATE_LAG_MS)
  }

  onMounted(async () => {
    if (!enabled) return
    os.value = detectDesktopOs()
    showCustomControls.value = os.value === 'windows' || os.value === 'linux'

    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const win = getCurrentWindow()
      dragWindow = win
      await refreshWindowState(win)
      unlistenResize = await win.onResized(() => {
        void refreshWindowState(win)
        scheduleFullscreenLagRefresh(win)
        scheduleMacosChromeRepair()
      })
    } catch (e) {
      console.warn('[window-chrome] init failed', e)
    }
  })

  onUnmounted(() => {
    unlistenResize?.()
    if (macosChromeRepairTimer) clearTimeout(macosChromeRepairTimer)
    if (fullscreenLagTimer) clearTimeout(fullscreenLagTimer)
  })

  async function runWindowAction(action: string, fn: () => Promise<void>) {
    try {
      await fn()
    } catch (e) {
      console.error(`[window-chrome] ${action} failed`, e)
    }
  }

  async function minimize() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    await runWindowAction('minimize', () => getCurrentWindow().minimize())
  }

  async function toggleMaximize() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    const win = getCurrentWindow()
    await runWindowAction('toggleMaximize', async () => {
      await win.toggleMaximize()
      await refreshWindowState(win)
      scheduleFullscreenLagRefresh(win)
      scheduleMacosChromeRepair()
    })
  }

  async function close() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    await runWindowAction('close', () => getCurrentWindow().close())
  }

  /** Must run synchronously during mousedown (macOS rejects late async calls). */
  function startDrag() {
    if (!enabled) return
    try {
      if (dragWindow) {
        void dragWindow.startDragging().catch(e => {
          console.error('[window-chrome] startDragging failed', e)
        })
        return
      }
      void import('@tauri-apps/api/window')
        .then(({ getCurrentWindow }) => {
          dragWindow = getCurrentWindow()
          return dragWindow.startDragging()
        })
        .catch(e => {
          console.error('[window-chrome] startDragging failed', e)
        })
    } catch (e) {
      console.error('[window-chrome] startDragging failed', e)
    }
  }

  return {
    enabled,
    os,
    maximized,
    fullscreen,
    showCustomControls,
    macTrafficLightPadding,
    minimize,
    toggleMaximize,
    close,
    startDrag
  }
}
