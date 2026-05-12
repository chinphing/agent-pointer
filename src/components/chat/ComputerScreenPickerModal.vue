<script setup lang="ts">
import { computed, watch } from 'vue'
import { X } from 'lucide-vue-next'
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
      class="fixed inset-0 z-[220] flex items-center justify-center bg-black/75 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="选择屏幕"
      @click.self="close"
    >
      <div class="relative w-full max-w-lg rounded-2xl glass-strong p-4 pt-12 shadow-2xl border border-white/10">
        <button
          type="button"
          class="absolute top-3 right-3 z-10 h-9 w-9 rounded-xl bg-white/10 hover:bg-white/15 text-slate-200 flex items-center justify-center cursor-pointer transition"
          title="关闭 (Esc)"
          @click="close"
        >
          <X class="w-4 h-4" />
        </button>

        <div class="pr-10">
          <div class="text-base text-slate-100 font-medium">选择要操作的屏幕</div>
          <div class="text-[12px] text-slate-400 mt-1 leading-relaxed">
            Computer Agent 会在该屏幕上截图、识别并执行鼠标键盘操作。
          </div>
        </div>

        <div v-if="error" class="mt-4 text-sm text-red-300">{{ error }}</div>
        <div v-else-if="loading" class="mt-4 text-sm text-slate-400">正在读取屏幕列表…</div>
        <div v-else class="mt-4 space-y-2">
          <button
            v-for="m in sorted"
            :key="m.id"
            type="button"
            class="w-full text-left rounded-xl border border-white/10 bg-white/5 hover:bg-white/8 transition px-3 py-2 cursor-pointer"
            @click="emit('pick', m.id)"
          >
            <div class="flex items-center justify-between gap-3">
              <div class="min-w-0">
                <div class="text-sm text-slate-200 truncate">
                  {{ m.isPrimary ? '主屏幕' : '屏幕' }}
                </div>
                <div class="text-[11px] text-slate-500 mt-0.5">
                  {{ m.width }}×{{ m.height }} · ({{ m.left }}, {{ m.top }})
                </div>
              </div>
              <div class="text-[10px] text-slate-600 shrink-0">
                {{ m.id }}
              </div>
            </div>
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
