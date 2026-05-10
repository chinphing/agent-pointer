<script setup lang="ts">
import { watch } from 'vue'
import { X } from 'lucide-vue-next'
import type { ComputerAnnotatedPreview } from '../../../../types/chat'

const open = defineModel<boolean>('open', { required: true })

defineProps<{
  loading: boolean
  preview: ComputerAnnotatedPreview | null
  error: string | null
}>()

function close() {
  open.value = false
}

function onPreviewKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

watch(
  open,
  v => {
    if (v) document.addEventListener('keydown', onPreviewKeydown)
    else document.removeEventListener('keydown', onPreviewKeydown)
  },
  { immediate: true }
)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[200] flex items-center justify-center bg-black/75 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="标注截图预览"
      @click.self="close"
    >
      <div
        class="relative max-w-[min(96vw,1280px)] max-h-[min(92vh,900px)] w-full overflow-auto rounded-2xl glass-strong p-4 pt-12 shadow-2xl border border-white/10"
      >
        <button
          type="button"
          class="absolute top-3 right-3 z-10 h-9 w-9 rounded-xl bg-white/10 hover:bg-white/15 text-slate-200 flex items-center justify-center cursor-pointer transition"
          title="关闭 (Esc)"
          @click="close"
        >
          <X class="w-4 h-4" />
        </button>
        <div v-if="error" class="text-sm text-red-300 pr-10">{{ error }}</div>
        <div v-else-if="loading" class="text-sm text-slate-400 pr-10">加载中…</div>
        <template v-else-if="preview">
          <p class="text-[11px] text-slate-400 mb-3 pr-10 leading-relaxed">{{ preview.caption }}</p>
          <img
            :src="`data:image/png;base64,${preview.imageBase64}`"
            alt="Annotated desktop"
            class="max-w-full h-auto rounded-xl border border-white/10 shadow-lg"
          />
        </template>
      </div>
    </div>
  </Teleport>
</template>
