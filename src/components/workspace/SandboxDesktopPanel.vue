<template>
  <!--
    全屏铺满整个窗口，但顶部 40px（2.5rem）让给窗口自己的控件，否则「已接管」那行
    会被红绿灯和窗口标题压住。让位方式分平台：
    · macOS：红绿灯是原生浮层，盖不住，所以用 pt-10 把内容整体下移（底色仍铺满）；
    · 其它平台：窗口按钮是应用自绘的，直接 top-10 保住标题栏，不然全屏时关不掉窗口。
  -->
  <div :class="fullscreen ? fullscreenClass : 'flex min-h-0 flex-1 flex-col'">
    <div
      class="desktop-toolbar flex shrink-0 items-center gap-2 border-b border-border px-3 py-1.5"
      :class="{ 'is-fullscreen': fullscreen }"
    >
      <Monitor class="h-3.5 w-3.5 shrink-0" :class="statusClass" />
      <span
        class="desktop-toolbar-status min-w-0 flex-1 truncate text-xs text-muted"
        :title="statusText"
        >{{ statusText }}</span
      >
      <!-- 图标按钮，和右边的重连/全屏保持一致；状态由 title 和配色表达 -->
      <button
        v-if="connected"
        type="button"
        class="workspace-tab desktop-toolbar-btn shrink-0"
        :class="{ 'text-accent': !viewOnly }"
        :title="viewOnly ? t('workspace.desktopTakeControl') : t('workspace.desktopReleaseControl')"
        @click="toggleControl"
      >
        <Hand v-if="!viewOnly" class="h-3.5 w-3.5" />
        <MousePointer v-else class="h-3.5 w-3.5" />
      </button>
      <button
        v-if="connected || state === 'error'"
        type="button"
        class="workspace-tab desktop-toolbar-btn shrink-0"
        :title="t('workspace.desktopReconnect')"
        @click="emit('reconnect')"
      >
        <RotateCw class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="workspace-tab desktop-toolbar-btn shrink-0"
        :title="fullscreen ? t('workspace.desktopExitFullscreen') : t('workspace.desktopFullscreen')"
        @click="toggleFullscreen"
      >
        <Minimize2 v-if="fullscreen" class="h-3.5 w-3.5" />
        <Maximize2 v-else class="h-3.5 w-3.5" />
      </button>
    </div>

    <div class="relative min-h-0 flex-1 overflow-hidden bg-black">
      <!-- noVNC 会往这个容器里插 canvas -->
      <!-- noVNC 用这个盒子的尺寸算缩放：整块桌面缩进面板，一屏看全。 -->
      <div ref="screenRef" class="desktop-screen absolute inset-0 overflow-hidden" />

      <!--
        默认只读：容器桌面常常正在被 agent 操作，直接接管会互相抢鼠标。
        点这个半透明按钮才把键盘鼠标交给虚拟机；Esc 或工具栏「释放」退回只读。
      -->
      <button
        v-if="connected && viewOnly"
        type="button"
        class="pointer-events-none absolute inset-0 grid place-items-center bg-transparent"
        @click="takeControl"
      >
        <!-- 只有胶囊本身可点：整块铺满会让宿主指针停在按钮上（cursor-pointer），
             把画面里的远程光标盖住。 -->
        <span
          class="pointer-events-auto cursor-pointer rounded-full bg-black/50 px-4 py-2 text-xs text-white/90 ring-1 ring-white/20 backdrop-blur-sm transition hover:bg-black/70"
        >
          {{ t('workspace.desktopTakeControl') }}
        </span>
      </button>

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
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Hand, Maximize2, Minimize2, Monitor, MousePointer, RotateCw } from 'lucide-vue-next'
import RFB from '@novnc/novnc/lib/rfb.js'
import { setSandboxDesktopActive } from '../../lib/api'
import { detectDesktopOs } from '../../lib/desktopOs'

/**
 * 面板内接管容器桌面。
 *
 * 只在被挂载时连接（父组件用 `v-if` 控制），离开标签页即断开——「接管」是一个
 * 明确的开关，不是常驻流。人和 agent 共用同一个 X 桌面，不仲裁（见设计 §7.5）。
 */
const props = defineProps<{
  wsUrl: string | null
  password: string | null
  conversationId: string
}>()

const emit = defineEmits<{ reconnect: [] }>()

const { t } = useI18n()

const screenRef = ref<HTMLElement | null>(null)
const state = ref<'connecting' | 'connected' | 'disconnected' | 'error'>('connecting')
const errorText = ref('')
const fullscreen = ref(false)
/** 默认只读：避免和 agent 抢鼠标键盘，点一下才接管。 */
const viewOnly = ref(true)

const isMac = detectDesktopOs() === 'macos'
const fullscreenClass = computed(() =>
  isMac
    ? 'fixed inset-0 z-50 flex flex-col bg-black pt-10'
    : 'fixed inset-x-0 bottom-0 top-10 z-50 flex flex-col bg-black'
)

let rfb: RFB | null = null

const connected = computed(() => state.value === 'connected')

const statusText = computed(() => {
  switch (state.value) {
    case 'connecting':
      return t('workspace.desktopConnecting')
    case 'connected':
      // 「已接管」指的是接管电脑（连上了远程桌面），不是键鼠已被接管——
      // 默认还是只读，键鼠状态单独说清楚。
      return viewOnly.value
        ? t('workspace.desktopConnected')
        : t('workspace.desktopConnectedControlling')
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
  // 整块桌面缩进面板，一屏看全（默认也是唯一视图，不再提供 1:1 切换）。
  // clipViewport/dragViewport 关掉：那条路要求按住鼠标拖动平移，只读态下拖不动。
  rfb.scaleViewport = true
  rfb.clipViewport = false
  rfb.dragViewport = false
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
  // 只读态不抢焦点；接管态才把键盘交给画布
  if (!viewOnly.value) rfb?.focus()
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
    instance.focusOnClick = !viewOnly.value
    instance.viewOnly = viewOnly.value
    rfb = instance
    applyScale()
  } catch (err) {
    state.value = 'error'
    errorText.value = t('workspace.desktopError')
    console.warn('sandbox desktop connect failed', err)
  }
}

function applyInput() {
  if (!rfb) return
  rfb.viewOnly = viewOnly.value
  rfb.focusOnClick = !viewOnly.value
  if (viewOnly.value) rfb.blur()
  else rfb.focus()
}

function takeControl() {
  viewOnly.value = false
  applyInput()
}

function releaseControl() {
  viewOnly.value = true
  applyInput()
}

function toggleControl() {
  viewOnly.value ? takeControl() : releaseControl()
}

/**
 * 全屏：把面板铺满应用窗口（CSS 覆盖层，不用 Fullscreen API——WKWebView 对
 * `requestFullscreen` 支持不稳）。Esc 退出。
 */
function toggleFullscreen() {
  fullscreen.value = !fullscreen.value
  // 尺寸变了 noVNC 要重新适配
  void nextTick(() => applyScale())
}

function onKeydown(event: KeyboardEvent) {
  // Esc 优先退出接管（把键盘还给 agent），再退全屏。
  if (event.key === 'Escape' && !viewOnly.value) {
    event.stopPropagation()
    releaseControl()
    return
  }
  if (event.key === 'Escape' && fullscreen.value) {
    event.stopPropagation()
    fullscreen.value = false
    void nextTick(() => applyScale())
  }
}

onMounted(() => {
  connect()
  // 接管期间别让空闲回收把容器干掉（面板轮询本身不刷新活跃时间）
  void setSandboxDesktopActive(props.conversationId, true)
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  teardown()
  window.removeEventListener('keydown', onKeydown)
  void setSandboxDesktopActive(props.conversationId, false)
})

// 容器重启后宿主端口会重新分配（`-p 127.0.0.1::6080` 是随机端口），
// 父组件重新解析端点后 URL 会变——这里跟着重连，别抱着死端口重试。
watch(
  () => props.wsUrl,
  (next, prev) => {
    if (next && next !== prev) connect()
  }
)
</script>

<style scoped>
/*
 * 全屏时面板底衬是纯黑，而 text-muted / border-border 是按主题表面配的灰，
 * 叠在黑底上几乎看不出边界——工具栏"像消失了"。全屏单独给一套高对比配色。
 */
.desktop-toolbar.is-fullscreen {
  border-bottom-color: rgba(255, 255, 255, 0.12);
  color: rgba(255, 255, 255, 0.75);
}
.desktop-toolbar.is-fullscreen .desktop-toolbar-status {
  color: rgba(255, 255, 255, 0.75);
}
.desktop-toolbar.is-fullscreen .desktop-toolbar-btn {
  color: rgba(255, 255, 255, 0.8);
}
/*
 * 光标由服务端画进画面（x11vnc -nocursorshape -cursor most），本地光标藏起来，
 * 否则鼠标停在画布上时会有两个指针，宿主那个还会盖住远程的。
 */
.desktop-screen :deep(canvas) {
  cursor: none;
}
.desktop-toolbar.is-fullscreen .desktop-toolbar-btn:hover {
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
}
</style>
