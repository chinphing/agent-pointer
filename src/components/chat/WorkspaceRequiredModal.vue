<script setup lang="ts">
import { watch } from 'vue'
import { FolderOpen, X } from 'lucide-vue-next'

const open = defineModel<boolean>('open', { required: true })

defineProps<{
  isDesktop: boolean
}>()

const emit = defineEmits<{
  (e: 'pick'): void
}>()

function close() {
  open.value = false
}

function onPick() {
  emit('pick')
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
      aria-label="请先选择工作目录"
      @click.self="close"
    >
      <div class="relative w-full max-w-md rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl overflow-hidden">
        <header class="flex items-start gap-3 px-5 pt-5 pb-3 border-b border-border">
          <div class="w-8 h-8 rounded-lg bg-warning/15 flex items-center justify-center shrink-0">
            <FolderOpen class="w-4 h-4 text-warning" />
          </div>
          <div class="min-w-0 flex-1 pr-8">
            <h2 class="text-base font-semibold text-foreground">请先选择工作目录</h2>
            <p class="text-[13px] text-muted mt-1 leading-relaxed">
              您需要选择一个目录存放你的代码。
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

        <footer class="flex justify-end gap-2 px-5 py-4 border-t border-border">
          <button
            type="button"
            class="h-9 px-4 rounded-lg bg-hover hover:opacity-90 text-sm text-foreground cursor-pointer transition-opacity"
            @click="close"
          >
            取消
          </button>
          <button
            type="button"
            class="h-9 px-4 rounded-lg bg-accent text-white text-sm font-medium cursor-pointer hover:opacity-95 transition-opacity"
            @click="onPick"
          >
            {{ isDesktop ? '选择目录' : '知道了' }}
          </button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>
