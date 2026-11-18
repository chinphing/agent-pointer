<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1.5">
      <Monitor class="h-3.5 w-3.5 shrink-0" :class="statusClass" />
      <span class="min-w-0 flex-1 truncate text-xs text-muted" :title="statusText">{{ statusText }}</span>
      <button
        v-if="connected"
        type="button"
        class="workspace-tab shrink-0"
        :title="fit ? t('workspace.desktopScaleActual') : t('workspace.desktopScaleFit')"
        @click="toggleScale"
      >
        {{ fit ? t('workspace.desktopScaleFitShort') : t('workspace.desktopScaleActualShort') }}
      </button>
      <button
        v-if="connected || state === 'error'"
        type="button"
        class="workspace-tab shrink-0"
        :title="t('workspace.desktopReconnect')"
        @click="reconnect"
      >
        <RotateCw class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="workspace-tab shrink-0"
        :title="t('workspace.desktopOpenExternal')"
        @click="$emit('open-external')"
      >
        <ExternalLink class="h-3.5 w-3.5" />
      </button>
    </div>

    <div class="relative min-h-0 flex-1 overflow-hidden bg-black">
      <!-- noVNC 会往这个容器里插 canvas -->
      <div ref="screenRef" class="absolute inset-0" />

      <div
        v-if="!connected"
        class="pointer-events-none absolute inset-0 grid place-items-center p-6 text-center"
      >
        <p class="max-w-72 text-xs leading-relaxed" :class="state === 'error' ? 'text-danger' : 'text-muted'">
          {{ statusText }}
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ExternalLink, Monitor, RotateCw } from 'lucide-vue-next'
import RFB from '@novnc/novnc/lib/rfb.js'

/**
 * 面板内接管容器桌面。
 *
 * 只在被挂载时连接（父组件用 `v-if` 控制），离开标签页即断开——「接管」是一个
 * 明确的开关，不是常驻流。人和 agent 共用同一个 X 桌面，不仲裁（见设计 §7.5）。
 */
const props = defineProps<{
  wsUrl: string | null
  password: string | null
}>()

defineEmits<{ 'open-external': [] }>()

const { t } = useI18n()

const screenRef = ref<HTMLElement | null>(null)
const state = ref<'connecting' | 'connected' | 'disconnected' | 'error'>('connecting')
const errorText = ref('')
const fit = ref(true)

let rfb: RFB | null = null

const connected = computed(() => state.value === 'connected')

const statusText = computed(() => {
  switch (state.value) {
    case 'connecting':
      return t('workspace.desktopConnecting')
    case 'connected':
      return fit.value ? t('workspace.desktopConnectedFit') : t('workspace.desktopConnectedActual')
    case 'error':
      return errorText.value || t('workspace.desktopError')
    default:
      return t('workspace.desktopDisconnected')
  }
})

const statusClass = computed(() => {
  if (state.value === 'connected') return 'text-accent'
  if (state.value === 'error') return 'text-danger'
  return 'text-muted'
})

function applyScale() {
  if (!rfb) return
  // 适应：整屏缩进面板；1:1：原尺寸 + 滚动平移（1920×1080 缩到 400px 宽只有 0.2 倍，字看不清）
  rfb.scaleViewport = fit.value
  rfb.clipViewport = !fit.value
  rfb.dragViewport = !fit.value
}

function teardown() {
  if (!rfb) return
  rfb.removeEventListener('connect', onConnect)
  rfb.removeEventListener('disconnect', onDisconnect)
  rfb.removeEventListener('securityfailure', onSecurityFailure)
  try {
    rfb.disconnect()
  } catch {
    // 已经断了
  }
  rfb = null
  if (screenRef.value) screenRef.value.innerHTML = ''
}

function onConnect() {
  state.value = 'connected'
  errorText.value = ''
  applyScale()
  // 连上就聚焦，键盘直接可用（不用先点一下）
  rfb?.focus()
}

function onDisconnect() {
  if (state.value === 'error') return
  state.value = 'disconnected'
}

function onSecurityFailure(event: Event) {
  const detail = (event as Event & { detail?: { reason?: string } }).detail
  state.value = 'error'
  errorText.value = t('workspace.desktopAuthFailed', { reason: detail?.reason || '-' })
}

function connect() {
  teardown()
  if (!props.wsUrl) {
    state.value = 'error'
    errorText.value = t('workspace.desktopUnsupported')
    return
  }
  const target = screenRef.value
  if (!target) return

  state.value = 'connecting'
  try {
    const instance = new RFB(target, props.wsUrl, {
      credentials: { password: props.password ?? '' },
    })
    instance.addEventListener('connect', onConnect)
    instance.addEventListener('disconnect', onDisconnect)
    instance.addEventListener('securityfailure', onSecurityFailure)
    instance.focusOnClick = true
    instance.viewOnly = false
    rfb = instance
    applyScale()
  } catch (err) {
    state.value = 'error'
    errorText.value = t('workspace.desktopError')
    console.warn('sandbox desktop connect failed', err)
  }
}

function reconnect() {
  connect()
}

function toggleScale() {
  fit.value = !fit.value
  applyScale()
}

onMounted(connect)
onBeforeUnmount(teardown)
</script>
