<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { Check, Keyboard, Monitor, X } from 'lucide-vue-next'
import type { MacosComputerPermissionsStatus } from '../../types/macosPermissions'
import type { MacosPermissionDragKind } from '../../lib/tauri'
import {
  beginMacosPermissionDragFlow,
  dismissMacosPermissionDragGuide,
  getMacosComputerPermissions,
  registerMacosScreenRecordingAccess
} from '../../lib/api'
import {
  clearMacosComputerPermissionsUserAck,
  setMacosComputerPermissionsUserAck
} from '../../lib/macosPermissionsSession'

const open = defineModel<boolean>('open', { required: true })

const emit = defineEmits<{
  (e: 'ready'): void
}>()

const STEPS = [
  {
    id: 'accessibility' as MacosPermissionDragKind,
    title: '辅助功能',
    icon: Keyboard
  },
  {
    id: 'screenRecording' as MacosPermissionDragKind,
    title: '屏幕录制',
    icon: Monitor
  }
] as const

const MANUAL_FALLBACK_DELAY_MS = 5000

const status = ref<MacosComputerPermissionsStatus | null>(null)
const busy = ref(false)
const error = ref<string | null>(null)
const activeDragKind = ref<MacosPermissionDragKind | null>(null)
const dragStarted = ref<Set<MacosPermissionDragKind>>(new Set())
const showManualFallback = ref<Set<MacosPermissionDragKind>>(new Set())
/** Wizard progress when user confirms drag but API still false */
const acknowledged = ref<Set<MacosPermissionDragKind>>(new Set())

const screenGranted = computed(() => status.value?.screenRecording === true)
const accessibilityGranted = computed(() => status.value?.accessibility === true)

function stepSystemGranted(id: MacosPermissionDragKind): boolean {
  return id === 'screenRecording' ? screenGranted.value : accessibilityGranted.value
}

function stepWizardDone(id: MacosPermissionDragKind): boolean {
  return stepSystemGranted(id) || acknowledged.value.has(id)
}

const allGranted = computed(
  () => screenGranted.value && accessibilityGranted.value
)
const allWizardDone = computed(() => STEPS.every(s => stepWizardDone(s.id)))

const completedCount = computed(
  () => STEPS.filter(s => stepWizardDone(s.id)).length
)

const progressPct = computed(() => (completedCount.value / 2) * 100)

const currentStepId = computed<MacosPermissionDragKind | 'done'>(() => {
  for (const s of STEPS) {
    if (!stepWizardDone(s.id)) return s.id
  }
  return 'done'
})

function stepState(id: MacosPermissionDragKind): 'done' | 'active' | 'pending' {
  if (stepWizardDone(id)) return 'done'
  if (currentStepId.value === id) return 'active'
  return 'pending'
}

let pollTimer: ReturnType<typeof setInterval> | null = null
let manualFallbackTimer: ReturnType<typeof setTimeout> | null = null

function clearManualFallbackTimer() {
  if (manualFallbackTimer) {
    clearTimeout(manualFallbackTimer)
    manualFallbackTimer = null
  }
}

function hideManualFallbackFor(id: MacosPermissionDragKind) {
  if (!showManualFallback.value.has(id)) return
  showManualFallback.value = new Set([...showManualFallback.value].filter(k => k !== id))
}

function scheduleManualFallback(kind: MacosPermissionDragKind) {
  clearManualFallbackTimer()
  hideManualFallbackFor(kind)
  manualFallbackTimer = setTimeout(() => {
    manualFallbackTimer = null
    if (stepSystemGranted(kind)) return
    if (stepState(kind) !== 'active') return
    showManualFallback.value = new Set([...showManualFallback.value, kind])
  }, MANUAL_FALLBACK_DELAY_MS)
}

function showSkipFallback(id: MacosPermissionDragKind): boolean {
  return (
    stepState(id) === 'active' &&
    dragStarted.value.has(id) &&
    showManualFallback.value.has(id) &&
    !stepSystemGranted(id)
  )
}

function showDetectingHint(id: MacosPermissionDragKind): boolean {
  return (
    stepState(id) === 'active' &&
    dragStarted.value.has(id) &&
    !showManualFallback.value.has(id) &&
    !stepSystemGranted(id)
  )
}

async function refresh() {
  const wasScreen = screenGranted.value
  const wasA11y = accessibilityGranted.value
  try {
    status.value = await getMacosComputerPermissions()
    error.value = null

    if (wasScreen === false && screenGranted.value) hideManualFallbackFor('screenRecording')
    if (wasA11y === false && accessibilityGranted.value) hideManualFallbackFor('accessibility')

    const screenJustGranted = !wasScreen && screenGranted.value
    const a11yJustGranted = !wasA11y && accessibilityGranted.value
    if (screenJustGranted || a11yJustGranted) {
      clearManualFallbackTimer()
      activeDragKind.value = null
      await dismissMacosPermissionDragGuide().catch(() => {})
    }

    if (allGranted.value) {
      clearMacosComputerPermissionsUserAck()
      stopPoll()
    }
  } catch (e: unknown) {
    error.value = String((e as Error)?.message || e)
  }
}

function startPoll() {
  stopPoll()
  pollTimer = setInterval(() => void refresh(), 350)
}

function stopPoll() {
  if (pollTimer) {
    clearInterval(pollTimer)
    pollTimer = null
  }
}

function close() {
  open.value = false
}

async function onLater() {
  clearManualFallbackTimer()
  await dismissMacosPermissionDragGuide().catch(() => {})
  activeDragKind.value = null
  close()
}

async function onManualComplete(kind: MacosPermissionDragKind) {
  busy.value = true
  error.value = null
  try {
    await refresh()
    if (!stepSystemGranted(kind)) {
      acknowledged.value = new Set([...acknowledged.value, kind])
    }
    clearManualFallbackTimer()
    hideManualFallbackFor(kind)
    activeDragKind.value = null
    await dismissMacosPermissionDragGuide().catch(() => {})
  } catch (e: unknown) {
    error.value = String((e as Error)?.message || e)
  } finally {
    busy.value = false
  }
}

async function onDragGrant(kind: MacosPermissionDragKind) {
  busy.value = true
  error.value = null
  activeDragKind.value = kind
  dragStarted.value = new Set([...dragStarted.value, kind])
  hideManualFallbackFor(kind)
  try {
    await beginMacosPermissionDragFlow(kind)
    startPoll()
    scheduleManualFallback(kind)
  } catch (e: unknown) {
    error.value = String((e as Error)?.message || e)
    activeDragKind.value = null
  } finally {
    busy.value = false
  }
}

function onContinue() {
  if (!allWizardDone.value) return
  if (allGranted.value) {
    clearMacosComputerPermissionsUserAck()
  } else {
    setMacosComputerPermissionsUserAck(true)
  }
  emit('ready')
  close()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') void onLater()
}

watch(currentStepId, id => {
  if (!open.value || id !== 'screenRecording' || screenGranted.value) return
  void registerMacosScreenRecordingAccess().catch(() => {})
})

watch(
  open,
  v => {
    if (v) {
      document.addEventListener('keydown', onKeydown)
      void refresh().then(() => {
        if (currentStepId.value === 'screenRecording' && !screenGranted.value) {
          void registerMacosScreenRecordingAccess().catch(() => {})
        }
        startPoll()
      })
    } else {
      document.removeEventListener('keydown', onKeydown)
      stopPoll()
      clearManualFallbackTimer()
      activeDragKind.value = null
      dragStarted.value = new Set()
      showManualFallback.value = new Set()
      acknowledged.value = new Set()
      void dismissMacosPermissionDragGuide().catch(() => {})
    }
  },
  { immediate: true }
)

onUnmounted(() => {
  stopPoll()
  clearManualFallbackTimer()
  document.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[230] flex items-center justify-center p-5"
      style="background: hsl(var(--foreground) / 0.36)"
      role="dialog"
      aria-modal="true"
      aria-label="电脑操控权限"
      @click.self="onLater"
    >
      <div
        class="relative w-full max-w-[400px] overflow-hidden rounded-2xl border border-border bg-card shadow-[0_20px_60px_hsl(var(--foreground)/0.15)]"
      >
        <div
          class="px-5 pt-5 pb-4 border-b border-border/80"
          style="background: linear-gradient(180deg, hsl(var(--accent) / 0.1), transparent)"
        >
          <button
            type="button"
            class="absolute top-3.5 right-3.5 p-2 rounded-lg text-muted hover:text-foreground hover:bg-hover cursor-pointer"
            title="关闭 (Esc)"
            @click="onLater"
          >
            <X class="w-4 h-4" />
          </button>
          <h2 class="text-base font-semibold text-foreground pr-8">电脑操控权限</h2>
          <p class="text-[12px] text-muted mt-1">拖拽 Pointer 到系统设置列表并保持启用</p>
          <div class="mt-3 h-1 rounded-full bg-hover overflow-hidden">
            <div
              class="h-full bg-accent transition-all duration-400"
              :style="{ width: `${progressPct}%` }"
            />
          </div>
          <p class="text-[11px] text-muted mt-1 tabular-nums">{{ completedCount }} / 2</p>
        </div>

        <div class="px-5 py-4 space-y-2">
          <div
            v-if="error"
            class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-2 text-[12px] text-danger"
            role="alert"
          >
            {{ error }}
          </div>

          <ul class="space-y-2" aria-label="授权步骤">
            <li
              v-for="(s, idx) in STEPS"
              :key="s.id"
              class="rounded-xl border px-4 py-3 transition-colors"
              :class="
                stepState(s.id) === 'active'
                  ? 'border-accent/40 bg-accent/[0.05]'
                  : stepState(s.id) === 'done'
                    ? 'border-border/50 bg-hover/25'
                    : 'border-border/60 opacity-70'
              "
            >
              <div class="flex items-center gap-3">
                <div
                  class="w-7 h-7 rounded-full flex items-center justify-center shrink-0 text-xs font-semibold"
                  :class="
                    stepState(s.id) === 'done'
                      ? 'bg-success/15 text-success'
                      : stepState(s.id) === 'active'
                        ? 'bg-accent text-accent-foreground'
                        : 'bg-hover text-muted'
                  "
                >
                  <Check v-if="stepState(s.id) === 'done'" class="w-3.5 h-3.5" />
                  <span v-else>{{ idx + 1 }}</span>
                </div>
                <component :is="s.icon" class="w-4 h-4 text-accent shrink-0" />
                <span class="text-sm font-medium text-foreground flex-1">{{ s.title }}</span>
                <span
                  v-if="stepState(s.id) === 'done'"
                  class="text-[11px] text-success shrink-0"
                >{{ stepSystemGranted(s.id) ? '完成' : '已确认' }}</span>
              </div>

              <template v-if="stepState(s.id) === 'active'">
                <div class="mt-3 pt-3 border-t border-border/50 space-y-2.5">
                  <p class="text-[12px] text-muted leading-relaxed">
                    点击按钮后，将左侧浮动卡片中的图标拖到系统设置列表，并保持启用。
                  </p>
                  <button
                    type="button"
                    class="w-full h-9 rounded-lg bg-accent text-accent-foreground text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50"
                    :disabled="busy"
                    @click="onDragGrant(s.id)"
                  >
                    {{ busy && activeDragKind === s.id ? '正在打开设置…' : '打开设置并拖拽' }}
                  </button>
                  <p
                    v-if="showDetectingHint(s.id)"
                    class="text-[11px] text-muted text-center"
                  >
                    正在检测授权…
                  </p>
                  <div
                    v-if="showSkipFallback(s.id)"
                    class="rounded-lg border border-dashed border-border bg-muted/5 px-3 py-2.5 space-y-2"
                  >
                    <p class="text-[11px] leading-snug">
                      <span class="font-medium text-accent">我已操作，但系统未检测到已授权</span>
                    </p>
                    <p class="text-[11px] text-muted leading-snug">
                      完全退出 Pointer 后从「应用程序」重新打开，检测通常会通过；也可先进入下一步继续设置。
                    </p>
                    <p
                      v-if="status && !status.runningFromAppBundle"
                      class="text-[10px] text-muted leading-snug break-all"
                    >
                      当前为开发运行路径，请在系统设置中授权此可执行文件，或使用打包后的 Pointer.app。
                    </p>
                    <button
                      type="button"
                      class="w-full h-8 rounded-md border border-border bg-card text-[13px] text-foreground hover:bg-hover cursor-pointer disabled:opacity-50"
                      :disabled="busy"
                      @click="onManualComplete(s.id)"
                    >
                      先进入下一步
                    </button>
                  </div>
                </div>
              </template>
            </li>
          </ul>

          <p
            v-if="allGranted"
            class="text-[13px] text-success text-center py-1"
          >
            权限已就绪
          </p>
          <p
            v-else-if="allWizardDone && !allGranted"
            class="text-[13px] text-muted text-center py-1 leading-relaxed"
          >
            步骤已确认，系统尚未全部通过。可点「仍要继续」发消息；操控异常请重启应用。
          </p>
        </div>

        <footer class="flex justify-between gap-2 px-5 py-3.5 border-t border-border">
          <button
            type="button"
            class="h-8 px-3 rounded-lg text-sm text-muted hover:bg-hover cursor-pointer"
            @click="onLater"
          >
            稍后再说
          </button>
          <button
            v-if="allWizardDone"
            type="button"
            class="h-8 px-4 rounded-lg bg-accent text-accent-foreground text-sm font-medium cursor-pointer hover:opacity-95"
            @click="onContinue"
          >
            {{ allGranted ? '继续' : '仍要继续' }}
          </button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>
