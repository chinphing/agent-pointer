<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Image as ImageIcon, Mic, Video, X } from 'lucide-vue-next'
import type { ComposerAttachment } from '../../types/chat'
import { getComposerAttachmentPreviewUrl } from '../../lib/attachmentPayloadStore'
import { videoPreviewUrlFromLocalPath, resolveVideoPreviewUrl } from '../../lib/chatMediaPreview'
import AttachmentFileIcon from './AttachmentFileIcon.vue'



const { t } = useI18n()
const props = defineProps<{
  attachment: ComposerAttachment
}>()

defineEmits<{
  remove: []
  cancel: []
  retry: []
}>()

const resolvedPreview = ref<string | null>(getComposerAttachmentPreviewUrl(props.attachment))
/** Blob URL created by this chip (web video stream preview); revoked on replace/unmount. */
let ownedPreviewUrl: string | null = null
/** Monotonic guard: stale in-flight previews must not clobber newer state. */
let previewSeq = 0

function revokeOwnedPreview() {
  if (ownedPreviewUrl) {
    URL.revokeObjectURL(ownedPreviewUrl)
    ownedPreviewUrl = null
  }
}

async function refreshVideoPreview() {
  const att = props.attachment
  if (att.kind !== 'video') return
  if (resolvedPreview.value) return
  const seq = ++previewSeq
  const url =
    (await resolveVideoPreviewUrl(att)) ??
    (att.localSourcePath ? await videoPreviewUrlFromLocalPath(att.localSourcePath) : null)
  if (seq !== previewSeq) {
    // A newer request (or a watch reset) superseded this one; drop its stale result.
    if (url?.startsWith('blob:')) URL.revokeObjectURL(url)
    return
  }
  if (!url) return
  if (ownedPreviewUrl && ownedPreviewUrl !== url) revokeOwnedPreview()
  if (url.startsWith('blob:')) ownedPreviewUrl = url
  resolvedPreview.value = url
}

onMounted(() => {
  void refreshVideoPreview()
})

onBeforeUnmount(revokeOwnedPreview)

watch(
  () => [props.attachment.storageRelPath, props.attachment.previewUrl, props.attachment.localSourcePath],
  () => {
    revokeOwnedPreview()
    previewSeq++ // invalidate any in-flight refresh before resetting the preview
    resolvedPreview.value = getComposerAttachmentPreviewUrl(props.attachment)
    void refreshVideoPreview()
  }
)

function previewUrl(att: ComposerAttachment): string | null {
  return resolvedPreview.value ?? getComposerAttachmentPreviewUrl(att)
}

function uploadLabel(att: ComposerAttachment): string | undefined {
  if (att.uploadState === 'compressing') return t('attachmentChip.compressing')
  if (att.uploadState === 'uploading' || att.uploadState === 'pending') {
    // Retry status text from Composer (locale-aware) takes precedence over progress %.
    if (att.uploadError) return att.uploadError
    const pct = att.uploadProgress ?? 0
    // 100% = bytes sent; server may still be saving / OSS — keep 100% visible.
    if (pct >= 100) return t('attachmentChip.processing100')
    return t('attachmentChip.uploading', { pct })
  }
  if (att.uploadState === 'error') return att.uploadError || t('attachmentChip.uploadFailed')
  if (att.uploadState === 'done') {
    if (att.kind === 'video' && att.remoteUrl) return t('attachmentChip.uploaded')
    if (att.storageRelPath) return t('attachmentChip.uploaded')
  }
  if (att.kind === 'video' && att.remoteUrl) return t('attachmentChip.uploaded')
  return undefined
}

const canCancelUpload = computed(() => {
  const s = props.attachment.uploadState
  return s === 'pending' || s === 'compressing' || s === 'uploading'
})

const canRetryUpload = computed(() => props.attachment.uploadState === 'error')
</script>

<template>
  <div
    class="inline-flex max-w-[220px] flex-col gap-1 rounded-lg border border-border bg-muted/40 px-2 py-1 text-xs text-foreground"
    :class="attachment.kind === 'video' && previewUrl(attachment) ? 'max-w-[240px]' : ''"
  >
    <div class="inline-flex items-center gap-1.5">
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
      <div
        v-else-if="attachment.kind === 'video' && previewUrl(attachment)"
        class="w-full max-w-[200px]"
      >
        <video
          :src="previewUrl(attachment)!"
          controls
          muted
          playsinline
          preload="metadata"
          class="w-full max-h-28 rounded object-contain"
        />
      </div>
      <Video v-else-if="attachment.kind === 'video'" class="h-4 w-4 shrink-0 text-muted" />
      <AttachmentFileIcon
        v-else
        :file-name="attachment.fileName"
        :mime-type="attachment.mimeType"
      />
      <span
        v-if="attachment.kind !== 'audio'"
        class="min-w-0 truncate"
        :title="attachment.fileName"
      >{{ attachment.fileName }}</span>
      <button
        type="button"
        class="shrink-0 rounded p-0.5 text-muted hover:bg-muted hover:text-foreground"
        :aria-label="t('attachmentChip.removeAria')"
        @click="$emit('remove')"
      >
        <X class="h-3.5 w-3.5" />
      </button>
    </div>
    <div
      v-if="uploadLabel(attachment)"
      class="w-full"
    >
      <div
        v-if="attachment.uploadState === 'compressing' || attachment.uploadState === 'uploading' || attachment.uploadState === 'pending'"
        class="h-1 w-full overflow-hidden rounded bg-muted"
      >
        <div
          class="h-full bg-primary transition-all duration-200"
          :style="{ width: `${attachment.uploadProgress ?? 0}%` }"
        />
      </div>
      <div class="flex items-center gap-2">
        <p
          class="min-w-0 flex-1 truncate text-[10px]"
          :class="attachment.uploadState === 'error' ? 'text-danger' : 'text-muted'"
          :title="uploadLabel(attachment)"
        >
          {{ uploadLabel(attachment) }}
        </p>
        <button
          v-if="canCancelUpload"
          type="button"
          class="shrink-0 text-[10px] text-muted underline-offset-2 hover:text-foreground hover:underline"
          @click="$emit('cancel')"
        >
          {{ t('attachmentChip.cancel') }}
        </button>
        <button
          v-if="canRetryUpload"
          type="button"
          class="shrink-0 text-[10px] text-primary underline-offset-2 hover:underline"
          @click="$emit('retry')"
        >
          {{ t('attachmentChip.retry') }}
        </button>
      </div>
    </div>
  </div>
</template>
