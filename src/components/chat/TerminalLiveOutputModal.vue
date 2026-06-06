<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'
import { Loader2, Terminal, X } from 'lucide-vue-next'

const props = defineProps<{
  command: string
  output: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const outputEl = ref<HTMLElement | null>(null)

const hasOutput = computed(() => props.output.trim().length > 0)

function scrollOutputToBottom() {
  const el = outputEl.value
  if (!el) return
  el.scrollTop = el.scrollHeight
}

function close() {
  emit('close')
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
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
      class="fixed inset-0 z-[240] flex items-center justify-center bg-black/55 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="终端命令输出"
      @click.self="close"
    >
      <div
        class="relative flex w-full max-w-2xl max-h-[min(80vh,32rem)] flex-col overflow-hidden rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl"
      >
        <header class="flex items-start gap-3 border-b border-border px-5 py-4 shrink-0 pr-12">
          <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-accent/15">
            <Terminal class="h-4 w-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <h2 class="text-base font-semibold text-foreground">终端输出</h2>
              <Loader2 class="h-3.5 w-3.5 shrink-0 animate-spin text-accent" aria-hidden="true" />
            </div>
            <p class="mt-1 text-[12px] text-muted leading-relaxed">
              命令已运行超过 3 秒，实时显示已捕获的输出；可手动关闭，命令仍在后台执行。
            </p>
          </div>
          <button
            type="button"
            class="absolute top-4 right-4 rounded-lg p-2 text-muted transition-colors hover:bg-hover cursor-pointer"
            aria-label="关闭"
            @click="close"
          >
            <X class="h-4 w-4" />
          </button>
        </header>

        <div class="min-h-0 flex-1 overflow-y-auto px-5 py-4 space-y-3">
          <div>
            <div class="mb-1 text-[10px] uppercase tracking-wider text-muted">执行命令</div>
            <pre class="text-[12px] font-mono whitespace-pre-wrap break-all rounded-lg border border-border bg-black/50 p-2.5 text-green-400">{{ command || '—' }}</pre>
          </div>
          <div>
            <div class="mb-1 text-[10px] uppercase tracking-wider text-muted">控制台输出</div>
            <pre
              ref="outputEl"
              class="text-[12px] font-mono whitespace-pre-wrap break-all rounded-lg border border-border bg-black/50 p-2.5 text-slate-200 max-h-[min(50vh,20rem)] overflow-y-auto"
            >{{ hasOutput ? output : '（暂无输出）' }}</pre>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
