<script setup lang="ts">
import { FileText, Image as ImageIcon, Mic, Video, X } from 'lucide-vue-next'
import type { ComposerAttachment } from '../../types/chat'
import { getComposerAttachmentPreviewUrl } from '../../lib/attachmentPayloadStore'

defineProps<{
  attachment: ComposerAttachment
}>()

defineEmits<{
  remove: []
}>()

function previewUrl(att: ComposerAttachment): string | null {
  return getComposerAttachmentPreviewUrl(att)
}
</script>

<template>
  <div
    class="inline-flex max-w-[200px] items-center gap-1.5 rounded-lg border border-border bg-muted/40 px-2 py-1 text-xs text-foreground"
  >
    <img
      v-if="attachment.kind === 'image' && previewUrl(attachment)"
      :src="previewUrl(attachment)!"
      :alt="attachment.fileName"
      class="h-8 w-8 shrink-0 rounded object-cover"
    />
    <ImageIcon
      v-else-if="attachment.kind === 'image'"
      class="h-4 w-4 shrink-0 text-muted"
    />
    <Mic v-else-if="attachment.kind === 'audio'" class="h-4 w-4 shrink-0 text-muted" />
    <video
      v-else-if="attachment.kind === 'video' && previewUrl(attachment)"
      :src="previewUrl(attachment)!"
      muted
      playsinline
      preload="metadata"
      class="h-8 w-8 shrink-0 rounded object-cover"
    />
    <Video v-else-if="attachment.kind === 'video'" class="h-4 w-4 shrink-0 text-muted" />
    <FileText v-else class="h-4 w-4 shrink-0 text-muted" />
    <span class="min-w-0 truncate" :title="attachment.fileName">{{ attachment.fileName }}</span>
    <button
      type="button"
      class="shrink-0 rounded p-0.5 text-muted hover:bg-muted hover:text-foreground"
      aria-label="移除附件"
      @click="$emit('remove')"
    >
      <X class="h-3.5 w-3.5" />
    </button>
  </div>
</template>
