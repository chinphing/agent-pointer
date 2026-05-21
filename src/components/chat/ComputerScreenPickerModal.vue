<script setup lang="ts">
import { computed, watch } from 'vue'
import { Monitor, X } from 'lucide-vue-next'
import type { ComputerMonitor } from '../../types/chat'

const open = defineModel<boolean>('open', { required: true })

const props = defineProps<{
  monitors: ComputerMonitor[]
  loading: boolean
  error: string | null
}>()

const emit = defineEmits<{
  (e: 'pick', monitorId: string): void
}>()

const sorted = computed(() => {
  return [...(props.monitors || [])].sort((a, b) => {
    if (a.isPrimary !== b.isPrimary) return a.isPrimary ? -1 : 1
    return a.id.localeCompare(b.id)
  })
})

function close() {
  open.value = false
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

watch(
  open,
  v => {
    if (v) document.addEventListener('keydown', onKeydown)
    else document.removeEventListener('keydown', onKeydown)
  },
  { immediate: true }
)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[220] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="选择屏幕"
      @click.self="close"
    >
      <div class="relative w-full max-w-lg rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl overflow-hidden">
        <header class="flex items-start gap-3 px-5 pt-5 pb-3 border-b border-border">
          <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
            <Monitor class="w-4 h-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1 pr-8">
            <h2 class="text-base font-semibold text-foreground">选择要操作的屏幕</h2>
            <p class="text-[12px] text-muted mt-0.5 leading-relaxed">
              Computer Agent 会在该屏幕上截图、识别并执行鼠标键盘操作。
            </p>
          </div>
          <button
            type="button"
            class="absolute top-4 right-4 p-2 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors"
            title="关闭 (Esc)"
            @click="close"
          >
            <X class="w-4 h-4" />
          </button>
        </header>

        <div class="p-5">
          <div v-if="error" class="rounded-xl border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">
            {{ error }}
          </div>
          <div v-else-if="loading" class="text-sm text-muted py-2">正在读取屏幕列表…</div>
          <div v-else class="space-y-2">
            <button
              v-for="m in sorted"
              :key="m.id"
              type="button"
              class="w-full text-left rounded-xl border border-border bg-card hover:bg-hover transition-colors px-3 py-2.5 cursor-pointer"
              @click="emit('pick', m.id)"
            >
              <div class="flex items-center justify-between gap-3">
                <div class="min-w-0">
                  <div class="text-sm font-medium text-foreground truncate">
                    {{ m.isPrimary ? '主屏幕' : '屏幕' }}
                  </div>
                  <div class="text-[11px] text-muted mt-0.5">
                    {{ m.width }}×{{ m.height }} · ({{ m.left }}, {{ m.top }})
                  </div>
                </div>
                <div class="text-[10px] text-muted font-mono shrink-0">
                  {{ m.id }}
                </div>
              </div>
            </button>
            <div v-if="!sorted.length" class="text-center text-sm text-muted py-6">
              未检测到可用屏幕
            </div>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
