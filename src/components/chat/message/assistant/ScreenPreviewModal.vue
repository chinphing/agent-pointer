<script setup lang="ts">
import { watch } from 'vue'
import { Image, X } from 'lucide-vue-next'
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
      class="fixed inset-0 z-[200] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="截图处理预览"
      @click.self="close"
    >
      <div
        class="relative max-w-[min(96vw,1280px)] max-h-[min(92vh,900px)] w-full overflow-auto rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl"
      >
        <header class="sticky top-0 z-10 flex items-center gap-3 px-5 py-4 border-b border-border bg-[hsl(var(--card-elevated))]">
          <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
            <Image class="w-4 h-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1 pr-8">
            <h2 class="text-sm font-semibold text-foreground">截图处理预览</h2>
            <p v-if="preview?.caption" class="text-[11px] text-muted mt-0.5 leading-relaxed line-clamp-2">
              {{ preview.caption }}
            </p>
          </div>
          <button
            type="button"
            class="absolute top-3 right-3 p-2 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors"
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
          <div v-else-if="loading" class="text-sm text-muted">加载中…</div>
          <img
            v-else-if="preview"
            :src="`data:${preview.imageMime ?? 'image/jpeg'};base64,${preview.imageBase64}`"
            alt="Annotated desktop"
            class="max-w-full h-auto rounded-xl border border-border shadow-lg"
          />
        </div>
      </div>
    </div>
  </Teleport>
</template>
