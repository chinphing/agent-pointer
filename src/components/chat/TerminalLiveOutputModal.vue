<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'
import { Maximize2, Minimize2, Terminal, X } from 'lucide-vue-next'

const props = defineProps<{
  command: string
  output: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const outputEl = ref<HTMLElement | null>(null)
const fullscreen = ref(false)

const hasOutput = computed(() => props.output.trim().length > 0)

function scrollOutputToBottom() {
  const el = outputEl.value
  if (!el) return
  el.scrollTop = el.scrollHeight
}

function close() {
  fullscreen.value = false
  emit('close')
}

function toggleFullscreen() {
  fullscreen.value = !fullscreen.value
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    if (fullscreen.value) fullscreen.value = false
    else close()
  }
}

watch(
  () => props.output,
  () => {
    void nextTick(scrollOutputToBottom)
  },
  { immediate: true }
)

document.addEventListener('keydown', onKeydown)
onUnmounted(() => document.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <div
      class="fixed inset-0 z-[240]"
      :class="
        fullscreen
          ? 'bg-[hsl(var(--card-elevated))]'
          : 'flex items-center justify-center bg-black/60 p-4'
      "
      role="dialog"
      aria-modal="true"
      aria-label="终端命令输出"
      @click.self="!fullscreen && close()"
    >
      <div
        class="relative flex flex-col overflow-hidden border border-border bg-[hsl(var(--card-elevated))] shadow-2xl"
        :class="
          fullscreen
            ? 'h-full w-full rounded-none border-0 shadow-none'
            : 'h-[70vh] max-h-[70vh] w-full max-w-2xl rounded-2xl'
        "
      >
        <header
          class="flex shrink-0 items-start gap-3 border-b border-border px-5 py-4"
          :class="fullscreen ? 'pr-24' : 'pr-20'"
        >
          <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-accent/15">
            <Terminal class="h-4 w-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <h2 class="text-base font-semibold text-foreground">终端输出</h2>
              <span
                class="inline-flex h-2 w-2 shrink-0 rounded-full bg-accent"
                aria-hidden="true"
                title="运行中"
              />
            </div>
            <p class="mt-1 text-[12px] text-muted leading-relaxed">
              命令已运行超过 5 秒且有输出时可查看；可手动关闭，命令仍在后台执行。
            </p>
          </div>
          <div class="absolute top-4 right-4 flex items-center gap-1">
            <button
              type="button"
              class="rounded-lg p-2 text-muted transition-colors hover:bg-hover cursor-pointer"
              :aria-label="fullscreen ? '退出全屏' : '全屏显示'"
              @click="toggleFullscreen"
            >
              <Minimize2 v-if="fullscreen" class="h-4 w-4" />
              <Maximize2 v-else class="h-4 w-4" />
            </button>
            <button
              type="button"
              class="rounded-lg p-2 text-muted transition-colors hover:bg-hover cursor-pointer"
              aria-label="关闭"
              @click="close"
            >
              <X class="h-4 w-4" />
            </button>
          </div>
        </header>

        <div class="flex min-h-0 flex-1 flex-col gap-3 px-5 py-4">
          <div class="shrink-0">
            <div class="mb-1 text-[10px] uppercase tracking-wider text-muted">执行命令</div>
            <pre class="max-h-64 overflow-y-auto text-[12px] font-mono whitespace-pre-wrap break-all rounded-lg border border-border bg-black/50 p-2.5 text-green-400">{{ command || '—' }}</pre>
          </div>
          <div class="flex min-h-0 flex-1 flex-col">
            <div class="mb-1 shrink-0 text-[10px] uppercase tracking-wider text-muted">控制台输出</div>
            <pre
              ref="outputEl"
              class="min-h-0 flex-1 overflow-y-auto text-[12px] font-mono whitespace-pre-wrap break-all rounded-lg border border-border bg-black/50 p-2.5 text-slate-200"
            >{{ hasOutput ? output : '（暂无输出）' }}</pre>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
