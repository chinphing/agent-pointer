import { ref, onMounted } from 'vue'
import { t } from '../i18n'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { isTauriRuntime } from '../lib/runtime'
import { usePlatformAuthStore } from '../stores/platformAuth'

const SKIPPED_VERSION_KEY = 'pointer.updater.skippedVersion'
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000 // 6 hours
const STARTUP_DELAY_MS = 30_000 // 30s after mount

/** Manual check: user has not confirmed download yet. */
const updateAvailable = ref(false)
/** Background path: package downloaded, waiting for install confirm. */
const updateReady = ref(false)
const updateVersion = ref<string | null>(null)
const updateNotes = ref<string | null>(null)
const checking = ref(false)
const downloading = ref(false)
/** Manual path: user confirmed; download + install in progress. */
const updating = ref(false)
const error = ref<string | null>(null)

const statusMessage = ref<string | null>(null)
let statusMessageTimer: ReturnType<typeof setTimeout> | null = null
let autoCheckStarted = false
let startupTimer: ReturnType<typeof setTimeout> | undefined
let intervalTimer: ReturnType<typeof setInterval> | undefined

function showStatusMessage(msg: string) {
  if (statusMessageTimer) clearTimeout(statusMessageTimer)
  statusMessage.value = msg
  statusMessageTimer = setTimeout(() => {
    statusMessage.value = null
  }, 5000)
}

function clearDiscoveryState() {
  updateAvailable.value = false
}

async function runCheck(): Promise<{
  available: boolean
  version?: string
  notes?: string
} | null> {
  if (!isTauriRuntime()) return null
  const platformAuth = usePlatformAuthStore()
  if (platformAuth.isStandalone) return null

  return invoke<{
    available: boolean
    version?: string
    notes?: string
  }>('check_for_update')
}

/** Settings → About →「检查更新」: check only, no download. */
async function checkForUpdateManual() {
  if (!isTauriRuntime()) return
  if (checking.value || updating.value || downloading.value) return

  const platformAuth = usePlatformAuthStore()
  if (platformAuth.isStandalone) return

  if (updateReady.value) {
    void invoke('updater_log', { msg: 'manual check skipped: update already downloaded' })
    return
  }

  checking.value = true
  error.value = null
  clearDiscoveryState()
  statusMessage.value = null

  const skipped = localStorage.getItem(SKIPPED_VERSION_KEY)
  try {
    const result = await runCheck()
    if (!result) return

    void invoke('updater_log', {
      msg: `manual check: available=${result.available} version=${result.version} skipped=${skipped}`,
    })

    if (!result.available || !result.version) {
      showStatusMessage(t('updater.upToDate'))
      return
    }
    if (skipped === result.version) {
      showStatusMessage(t('updater.skippedVersion', { version: result.version }))
      return
    }

    updateVersion.value = result.version
    updateNotes.value = result.notes ?? null
    updateAvailable.value = true
  } catch (e) {
    const errMsg = typeof e === 'string' ? e : String(e)
    void invoke('updater_log', { msg: `manual check failed: ${errMsg}` })
    error.value = errMsg
  } finally {
    checking.value = false
  }
}

/** Background: silent check + download; install deferred until user confirms. */
async function backgroundCheckAndDownload() {
  if (!isTauriRuntime()) return
  if (updateReady.value || updateAvailable.value || downloading.value || checking.value || updating.value) {
    void invoke('updater_log', {
      msg: `skip background check: ready=${updateReady.value} available=${updateAvailable.value} downloading=${downloading.value} checking=${checking.value} updating=${updating.value}`,
    })
    return
  }

  const platformAuth = usePlatformAuthStore()
  if (platformAuth.isStandalone) return

  const skipped = localStorage.getItem(SKIPPED_VERSION_KEY)
  checking.value = true
  error.value = null
  try {
    const result = await runCheck()
    if (!result) return

    void invoke('updater_log', {
      msg: `background check: available=${result.available} version=${result.version} skipped=${skipped}`,
    })

    if (!result.available || !result.version) return
    if (skipped === result.version) return

    updateVersion.value = result.version
    updateNotes.value = result.notes ?? null

    const pendingVersion = await invoke<string | null>('pending_update_version')
    if (pendingVersion === result.version) {
      void invoke('updater_log', { msg: `reuse pending download for ${result.version}` })
      updateReady.value = true
      return
    }

    downloading.value = true
    void invoke('updater_log', { msg: 'background download_update (install deferred)' })
    await invoke('download_update')
    void invoke('updater_log', { msg: 'background download succeeded, waiting for user confirm' })
    updateReady.value = true
  } catch (e) {
    const errMsg = typeof e === 'string' ? e : String(e)
    void invoke('updater_log', { msg: `background check/download failed: ${errMsg}` })
    // Background failures stay silent in UI; log only.
  } finally {
    checking.value = false
    downloading.value = false
  }
}

/**
 * User confirmed「立即更新」.
 * - Manual discovery: download + install + auto relaunch.
 * - Background ready: install cached package + auto relaunch.
 */
async function applyUpdateNow() {
  if (!isTauriRuntime()) return
  error.value = null

  try {
    if (updateReady.value) {
      void invoke('updater_log', { msg: 'user confirmed install (background ready)' })
      await invoke('install_and_restart')
      return
    }

    if (!updateAvailable.value || !updateVersion.value) return

    updating.value = true
    void invoke('updater_log', { msg: 'user confirmed download_and_install (manual)' })
    await invoke('download_update')
    await invoke('install_and_restart')
  } catch (e) {
    const errMsg = typeof e === 'string' ? e : String(e)
    void invoke('updater_log', { msg: `applyUpdateNow failed: ${errMsg}` })
    error.value = errMsg
  } finally {
    updating.value = false
  }
}

function dismissAvailable() {
  clearDiscoveryState()
}

function dismissReady() {
  updateReady.value = false
}

function skipVersion() {
  if (updateVersion.value) {
    localStorage.setItem(SKIPPED_VERSION_KEY, updateVersion.value)
  }
  clearDiscoveryState()
  updateReady.value = false
}

function startAutoCheckIfNeeded() {
  if (autoCheckStarted) return
  autoCheckStarted = true
  startupTimer = setTimeout(() => void backgroundCheckAndDownload(), STARTUP_DELAY_MS)
  intervalTimer = setInterval(() => void backgroundCheckAndDownload(), CHECK_INTERVAL_MS)
}

export function useAppUpdater() {
  onMounted(async () => {
    if (!isTauriRuntime()) return
    const platformAuth = usePlatformAuthStore()
    if (platformAuth.isStandalone) return

    await listen<{ downloaded: number; total: number | null }>(
      'updater://download-progress',
      () => {}
    )

    startAutoCheckIfNeeded()
  })

  return {
    updateAvailable,
    updateReady,
    updateVersion,
    updateNotes,
    checking,
    downloading,
    updating,
    error,
    statusMessage,
    checkForUpdateManual,
    applyUpdateNow,
    dismissAvailable,
    dismissReady,
    skipVersion,
  }
}
