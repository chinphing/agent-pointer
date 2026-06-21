<script setup lang="ts">
import { ref } from 'vue'
import { Monitor } from 'lucide-vue-next'
import ScreenPreviewModal from '../chat/message/assistant/ScreenPreviewModal.vue'
import { captureManualDesktopSnapshot } from '../../lib/web'
import type { ComputerAnnotatedPreview } from '../../types/chat'

const open = ref(false)
const loading = ref(false)
const preview = ref<ComputerAnnotatedPreview | null>(null)
const error = ref<string | null>(null)

async function onClick() {
  open.value = true
  loading.value = true
  preview.value = null
  error.value = null
  try {
    preview.value = await captureManualDesktopSnapshot()
  } catch (e) {
    error.value = e instanceof Error ? e.message : '截图失败'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <button
    type="button"
    class="chrome-icon-btn"
    title="查看桌面"
    @click="onClick"
  >
    <Monitor class="w-4 h-4" />
  </button>
  <ScreenPreviewModal
    v-model:open="open"
    :loading="loading"
    :preview="preview"
    :error="error"
  />
</template>
