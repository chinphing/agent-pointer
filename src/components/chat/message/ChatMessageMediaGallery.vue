<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { FileText } from 'lucide-vue-next'
import type { RenderableAttachment } from '../../../lib/messageNormalizer'
import { previewChatMedia, previewMediaRef } from '../../../lib/api'

const props = defineProps<{
  attachments: RenderableAttachment[]
  align?: 'start' | 'end'
}>()

const loadedPreviews = ref<Record<string, string>>({})

async function ensureMediaPreview(att: RenderableAttachment) {
  if (loadedPreviews.value[att.id] || att.previewUrl) return
  try {
    let preview
    if (att.mediaRef) {
      preview = await previewMediaRef(att.mediaRef)
    } else if (att.localAbsPath) {
      preview = await previewMediaRef(att.localAbsPath)
    } else if (att.storageRelPath) {
      preview = await previewChatMedia(att.storageRelPath)
    } else {
      return
    }
    const mime = preview.mimeType || 'application/octet-stream'
    loadedPreviews.value = {
      ...loadedPreviews.value,
      [att.id]: `data:${mime};base64,${preview.dataBase64}`
    }
  } catch (e) {
    console.warn('media preview failed', att.fileName, e)
  }
}

function mediaSrc(att: RenderableAttachment): string | null {
  if (att.previewUrl) return att.previewUrl
  return loadedPreviews.value[att.id] ?? null
}

const alignClass = computed(() =>
  props.align === 'end' ? 'items-end' : 'items-start'
)

watch(
  () => props.attachments,
  list => {
    for (const att of list) void ensureMediaPreview(att)
  },
  { immediate: true, deep: true }
)

onMounted(() => {
  for (const att of props.attachments) void ensureMediaPreview(att)
})
</script>

<template>
  <div
    v-if="attachments.length"
    class="flex flex-col gap-2 w-full"
    :class="alignClass"
  >
    <template v-for="att in attachments" :key="att.id">
      <img
        v-if="att.kind === 'image' && mediaSrc(att)"
        :src="mediaSrc(att)!"
        :alt="att.fileName"
        class="max-h-64 max-w-full rounded-xl border border-border object-contain"
      />
      <div
        v-else-if="att.kind === 'audio'"
        class="w-full max-w-sm rounded-xl border border-border bg-muted/30 px-3 py-2"
      >
        <p class="text-[11px] text-muted mb-1 truncate" :title="att.fileName">{{ att.fileName }}</p>
        <audio
          v-if="mediaSrc(att)"
          controls
          preload="metadata"
          class="w-full"
          :src="mediaSrc(att)!"
        />
      </div>
      <div
        v-else-if="att.kind === 'video'"
        class="w-full max-w-sm rounded-xl border border-border bg-muted/30 px-3 py-2"
      >
        <p class="text-[11px] text-muted mb-1 truncate" :title="att.fileName">{{ att.fileName }}</p>
        <video
          v-if="mediaSrc(att)"
          controls
          preload="metadata"
          class="w-full max-h-64 rounded-lg"
          :src="mediaSrc(att)!"
        />
      </div>
      <div
        v-else
        class="inline-flex items-center gap-2 rounded-xl border border-border bg-muted/30 px-3 py-2 text-xs text-foreground"
      >
        <FileText class="h-4 w-4 shrink-0 text-muted" />
        <span class="truncate max-w-[240px]" :title="att.fileName">{{ att.fileName }}</span>
      </div>
    </template>
  </div>
</template>
