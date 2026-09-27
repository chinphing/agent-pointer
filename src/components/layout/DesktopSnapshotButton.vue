<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { Monitor } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import ScreenPreviewModal from '../chat/message/assistant/ScreenPreviewModal.vue'
import { captureManualDesktopSnapshot } from '../../lib/web'

const { t } = useI18n()
const SNAPSHOT_REFRESH_MS = 5000
const caption = computed(() => t('shell.currentDesktop'))

const open = ref(false)
const loading = ref(false)
const imageUrl = ref<string | null>(null)
const error = ref<string | null>(null)

let refreshTimer: ReturnType<typeof setInterval> | null = null
let refreshGen = 0
let inFlight = false

function revokeImageUrl() {
  if (imageUrl.value?.startsWith('blob:')) {
    URL.revokeObjectURL(imageUrl.value)
  }
  imageUrl.value = null
}

function stopRefresh() {
  if (refreshTimer) {
    clearInterval(refreshTimer)
    refreshTimer = null
  }
}

function startRefresh() {
  stopRefresh()
  refreshTimer = setInterval(() => void refreshSnapshot(true), SNAPSHOT_REFRESH_MS)
}

async function refreshSnapshot(isBackground = false) {
  if (inFlight) return
  const gen = ++refreshGen
  inFlight = true
  if (!isBackground) {
    loading.value = true
    revokeImageUrl()
    error.value = null
  }
  try {
    const blob = await captureManualDesktopSnapshot()
    if (gen !== refreshGen || !open.value) return
    const url = URL.createObjectURL(blob)
    revokeImageUrl()
    imageUrl.value = url
    error.value = null
  } catch (e) {
    if (gen !== refreshGen || !open.value) return
    const msg = e instanceof Error ? e.message : t('shell.screenshotFailed')
    if (!isBackground || !imageUrl.value) {
      error.value = msg
    } else {
      console.warn('[desktop snapshot] refresh failed:', msg)
    }
  } finally {
    inFlight = false
    if (!isBackground && gen === refreshGen) loading.value = false
  }
}

function onClick() {
  open.value = true
}

watch(open, v => {
  if (v) {
    void refreshSnapshot(false)
    startRefresh()
  } else {
    refreshGen++
    stopRefresh()
    revokeImageUrl()
  }
})

onUnmounted(() => {
  refreshGen++
  stopRefresh()
  revokeImageUrl()
})
</script>

<template>
  <button
    type="button"
    class="chrome-icon-btn"
    :title="t('shell.viewDesktop')"
    @click="onClick"
  >
    <Monitor class="w-4 h-4" />
  </button>
  <ScreenPreviewModal
    v-model:open="open"
    :loading="loading"
    :preview="null"
    :image-src="imageUrl"
    :caption="caption"
    :error="error"
  />
</template>
