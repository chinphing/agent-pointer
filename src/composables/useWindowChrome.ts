import { onMounted, onUnmounted, ref } from 'vue'
import { detectDesktopOs, type DesktopOs } from '../lib/desktopOs'
import { isTauriRuntime } from '../lib/runtime'

export type { DesktopOs }

/** Native traffic lights (macOS overlay); custom buttons on Windows/Linux. */
export function useWindowChrome() {
  const enabled = isTauriRuntime()
  const os = ref<DesktopOs>('unknown')
  const maximized = ref(false)

  const showCustomControls = ref(false)
  const macTrafficLightPadding = ref(false)

  let unlisten: (() => void) | undefined

  onMounted(async () => {
    if (!enabled) return
    os.value = detectDesktopOs()
    macTrafficLightPadding.value = os.value === 'macos'
    showCustomControls.value = os.value === 'windows' || os.value === 'linux'

    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const win = getCurrentWindow()
      maximized.value = await win.isMaximized()
      unlisten = await win.onResized(async () => {
        maximized.value = await win.isMaximized()
      })
    } catch (e) {
      console.warn('[window-chrome] init failed', e)
    }
  })

  onUnmounted(() => {
    unlisten?.()
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
    })
  }

  async function close() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    await runWindowAction('close', () => getCurrentWindow().close())
  }

  async function startDrag() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    await runWindowAction('startDragging', () => getCurrentWindow().startDragging())
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
