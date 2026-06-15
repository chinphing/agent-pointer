import { onMounted, onUnmounted, ref } from 'vue'
import { detectDesktopOs, type DesktopOs } from '../lib/desktopOs'
import { reapplyWindowChrome } from '../lib/api'
import { isTauriRuntime } from '../lib/runtime'

const MACOS_CHROME_REPAIR_DEBOUNCE_MS = 400

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
  const os = ref<DesktopOs>('unknown')
  const maximized = ref(false)

  const showCustomControls = ref(false)
  const macTrafficLightPadding = ref(false)

  let unlistenResize: (() => void) | undefined
  let macosChromeRepairTimer: ReturnType<typeof setTimeout> | undefined

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

  onMounted(async () => {
    if (!enabled) return
    os.value = detectDesktopOs()
    macTrafficLightPadding.value = os.value === 'macos'
    showCustomControls.value = os.value === 'windows' || os.value === 'linux'

    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const win = getCurrentWindow()
      dragWindow = win
      maximized.value = await win.isMaximized()
      unlistenResize = await win.onResized(async () => {
        maximized.value = await win.isMaximized()
        scheduleMacosChromeRepair()
      })
    } catch (e) {
      console.warn('[window-chrome] init failed', e)
    }
  })

  onUnmounted(() => {
    unlistenResize?.()
    if (macosChromeRepairTimer) clearTimeout(macosChromeRepairTimer)
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
      maximized.value = await win.isMaximized()
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
    showCustomControls,
    macTrafficLightPadding,
    minimize,
    toggleMaximize,
    close,
    startDrag
  }
}
