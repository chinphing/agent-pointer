<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { Check, Keyboard, Monitor, X } from 'lucide-vue-next'
import type { MacosComputerPermissionsStatus } from '../../types/macosPermissions'
import type { MacosPermissionDragKind } from '../../lib/tauri'
import {
  beginMacosPermissionDragFlow,
  dismissMacosPermissionDragGuide,
  getMacosComputerPermissions
} from '../../lib/tauri'

const open = defineModel<boolean>('open', { required: true })

const emit = defineEmits<{
  (e: 'ready'): void
}>()

const STEPS = [
  {
    id: 'screenRecording' as MacosPermissionDragKind,
    title: '屏幕录制',
    icon: Monitor
  },
  {
    id: 'accessibility' as MacosPermissionDragKind,
    title: '辅助功能',
    icon: Keyboard
  }
] as const

const status = ref<MacosComputerPermissionsStatus | null>(null)
const busy = ref(false)
const error = ref<string | null>(null)
const activeDragKind = ref<MacosPermissionDragKind | null>(null)
const waitingHint = ref<string | null>(null)

const screenDone = computed(() => status.value?.screenRecording === true)
const accessibilityDone = computed(() => status.value?.accessibility === true)
const allDone = computed(() => screenDone.value && accessibilityDone.value)

const completedCount = computed(
  () => (screenDone.value ? 1 : 0) + (accessibilityDone.value ? 1 : 0)
)

const progressPct = computed(() => (completedCount.value / 2) * 100)

const currentStepId = computed<MacosPermissionDragKind | 'done'>(() => {
  if (!screenDone.value) return 'screenRecording'
  if (!accessibilityDone.value) return 'accessibility'
  return 'done'
})

function stepState(id: MacosPermissionDragKind): 'done' | 'active' | 'pending' {
  if (id === 'screenRecording') {
    if (screenDone.value) return 'done'
    if (currentStepId.value === 'screenRecording') return 'active'
    return 'pending'
  }
  if (accessibilityDone.value) return 'done'
  if (currentStepId.value === 'accessibility') return 'active'
  return 'pending'
}

let pollTimer: ReturnType<typeof setInterval> | null = null
let waitingTimer: ReturnType<typeof setTimeout> | null = null

function clearWaitingTimer() {
  if (waitingTimer) {
    clearTimeout(waitingTimer)
    waitingTimer = null
  }
}

function scheduleWaitingHint(kind: MacosPermissionDragKind) {
  clearWaitingTimer()
  waitingTimer = setTimeout(() => {
    if (activeDragKind.value !== kind) return
    const granted =
      kind === 'screenRecording' ? screenDone.value : accessibilityDone.value
    if (granted) return
    const preflight = status.value?.screenRecordingPreflight
    const effective = kind === 'screenRecording' ? screenDone.value : accessibilityDone.value
    if (effective) return
    if (kind === 'screenRecording' && preflight === false) {
      waitingHint.value =
        '系统设置里若已启用仍停在此步：正在用截图检测权限。若超过 10 秒仍无反应，请完全退出 Pointer 后从「应用程序」重新打开（勿从 DMG 内双击）。'
    } else if (kind === 'accessibility') {
      waitingHint.value =
        '辅助功能在设置里已启用后，有时需完全退出并重新打开 Pointer 才会生效。'
    }
  }, 6000)
}

async function refresh() {
  const wasScreen = screenDone.value
  const wasA11y = accessibilityDone.value
  try {
    status.value = await getMacosComputerPermissions()
    error.value = null

    const screenJustGranted = !wasScreen && screenDone.value
    const a11yJustGranted = !wasA11y && accessibilityDone.value
    if (screenJustGranted || a11yJustGranted) {
      waitingHint.value = null
      clearWaitingTimer()
      activeDragKind.value = null
      await dismissMacosPermissionDragGuide().catch(() => {})
    }

    if (allDone.value) {
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
  clearWaitingTimer()
  waitingHint.value = null
  await dismissMacosPermissionDragGuide().catch(() => {})
  activeDragKind.value = null
  close()
}

async function onDragGrant(kind: MacosPermissionDragKind) {
  busy.value = true
  error.value = null
  waitingHint.value = null
  activeDragKind.value = kind
  try {
    await beginMacosPermissionDragFlow(kind)
    startPoll()
    scheduleWaitingHint(kind)
  } catch (e: unknown) {
    error.value = String((e as Error)?.message || e)
    activeDragKind.value = null
  } finally {
    busy.value = false
  }
}

function onContinue() {
  if (!allDone.value) return
  emit('ready')
  close()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') void onLater()
}

watch(
  open,
  v => {
    if (v) {
      document.addEventListener('keydown', onKeydown)
      void refresh().then(() => startPoll())
    } else {
      document.removeEventListener('keydown', onKeydown)
      stopPoll()
      clearWaitingTimer()
      waitingHint.value = null
      activeDragKind.value = null
      void dismissMacosPermissionDragGuide().catch(() => {})
    }
  },
  { immediate: true }
)

onUnmounted(() => {
  stopPoll()
  clearWaitingTimer()
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

          <div
            v-if="waitingHint && activeDragKind"
            class="rounded-lg border border-accent/25 bg-accent/5 px-3 py-2 text-[12px] text-foreground leading-relaxed"
          >
            {{ waitingHint }}
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
                        ? 'bg-accent text-white'
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
                  class="text-[11px] text-success"
                >完成</span>
              </div>

              <template v-if="stepState(s.id) === 'active'">
                <p class="text-[12px] text-muted mt-2 pl-10">
                  点击按钮后，将左侧浮动卡片中的图标拖到系统设置列表（保持启用）。
                </p>
                <button
                  type="button"
                  class="mt-2.5 ml-10 mr-0 w-[calc(100%-2.5rem)] h-9 rounded-lg bg-accent text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50"
                  :disabled="busy"
                  @click="onDragGrant(s.id)"
                >
                  {{ busy && activeDragKind === s.id ? '正在打开设置…' : '打开设置并拖拽' }}
                </button>
              </template>
            </li>
          </ul>

          <p
            v-if="allDone"
            class="text-[13px] text-success text-center py-1"
          >
            权限已就绪
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
            v-if="allDone"
            type="button"
            class="h-8 px-4 rounded-lg bg-accent text-white text-sm font-medium cursor-pointer hover:opacity-95"
            @click="onContinue"
          >
            继续
          </button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>
