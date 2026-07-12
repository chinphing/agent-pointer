import { ref, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { isTauriRuntime } from '../lib/runtime'
import { usePlatformAuthStore } from '../stores/platformAuth'

const SKIPPED_VERSION_KEY = 'pointer.updater.skippedVersion'
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000 // 6 hours
const STARTUP_DELAY_MS = 30_000 // 30s after mount

export function useAppUpdater() {
  const updateReady = ref(false)
  const updateVersion = ref<string | null>(null)
  const updateNotes = ref<string | null>(null)
  const checking = ref(false)
  const downloading = ref(false)
  const error = ref<string | null>(null)

  const platformAuth = usePlatformAuthStore()

  async function checkAndDownload() {
    if (!isTauriRuntime()) return
    // Only check on platform mode — standalone has no official update server
    if (platformAuth.isStandalone) return

    const skipped = localStorage.getItem(SKIPPED_VERSION_KEY)
    checking.value = true
    error.value = null
    try {
      const result = await invoke<{
        available: boolean
        version?: string
        notes?: string
      }>('check_for_update')
      if (!result.available || !result.version) return
      if (skipped === result.version) return
      updateVersion.value = result.version
      updateNotes.value = result.notes ?? null
      downloading.value = true
      await invoke('download_update')
      updateReady.value = true
    } catch (e) {
      error.value = String(e)
      console.warn('[updater] check/download failed', e)
    } finally {
      checking.value = false
      downloading.value = false
    }
  }

  async function relaunch() {
    await invoke('restart_app')
  }

  function dismiss() {
    updateReady.value = false
  }

  function skipVersion() {
    if (updateVersion.value) {
      localStorage.setItem(SKIPPED_VERSION_KEY, updateVersion.value)
    }
    updateReady.value = false
  }

  let startupTimer: ReturnType<typeof setTimeout> | undefined
  let intervalTimer: ReturnType<typeof setInterval> | undefined

  onMounted(async () => {
    if (!isTauriRuntime()) return
    if (platformAuth.isStandalone) return

    await listen<{ downloaded: number; total: number | null }>(
      'updater://download-progress',
      () => {
        // progress tracking; silent in background
      }
    )

    startupTimer = setTimeout(() => void checkAndDownload(), STARTUP_DELAY_MS)
    intervalTimer = setInterval(() => void checkAndDownload(), CHECK_INTERVAL_MS)
  })

  onUnmounted(() => {
    if (startupTimer) clearTimeout(startupTimer)
    if (intervalTimer) clearInterval(intervalTimer)
  })

  return {
    updateReady,
    updateVersion,
    updateNotes,
    checking,
    downloading,
    error,
    checkAndDownload,
    relaunch,
    dismiss,
    skipVersion,
  }
}
