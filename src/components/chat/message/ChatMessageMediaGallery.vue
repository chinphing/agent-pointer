<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Clipboard, FileText, FolderOpen } from 'lucide-vue-next'
import type { RenderableAttachment } from '../../../lib/messageNormalizer'
import ChatAudioPlayer from './ChatAudioPlayer.vue'
import ChatAudioTranscript from './ChatAudioTranscript.vue'
import { previewChatMedia, previewMediaRef, revealInFinder } from '../../../lib/api'
import { prefetchAttachmentAbsPaths, resolveAttachmentAbsPath } from '../../../lib/attachmentLocalPath'
import { resolveVideoPreviewUrl } from '../../../lib/chatMediaPreview'
import { isUsableAttachmentPreviewUrl } from '../../../lib/attachmentSupport'
import {
  isOpenableFileAttachment,
  openAttachmentWithSystemDefault
} from '../../../lib/openAttachment'
import { isTauriRuntime } from '../../../lib/runtime'

const props = defineProps<{
  attachments: RenderableAttachment[]
  align?: 'start' | 'end'
}>()

const loadedPreviews = ref<Record<string, string>>({})
const resolvedAbsPaths = ref<Record<string, string>>({})
const previewInflight = new Set<string>()

async function ensureMediaPreview(att: RenderableAttachment) {
  if (loadedPreviews.value[att.id] || previewInflight.has(att.id)) return
  previewInflight.add(att.id)
  try {
    if (att.kind === 'video') {
      const streamUrl = await resolveVideoPreviewUrl(att)
      if (streamUrl) {
        loadedPreviews.value = { ...loadedPreviews.value, [att.id]: streamUrl }
        return
      }
    }
    let preview
    if (att.mediaRef && !att.storageRelPath) {
      preview = await previewMediaRef(att.mediaRef)
    } else if (att.localAbsPath) {
      preview = await previewMediaRef(att.localAbsPath)
    } else if (att.storageRelPath) {
      preview = await previewChatMedia(att.storageRelPath)
    } else if (att.mediaRef) {
      preview = await previewMediaRef(att.mediaRef)
    } else {
      return
    }
    const mime =
      preview.mimeType && preview.mimeType !== 'application/octet-stream'
        ? preview.mimeType
        : att.mimeType || preview.mimeType || 'application/octet-stream'
    loadedPreviews.value = {
      ...loadedPreviews.value,
      [att.id]: `data:${mime};base64,${preview.dataBase64}`
    }
  } catch (e) {
    console.warn('media preview failed', att.fileName, e)
  } finally {
    previewInflight.delete(att.id)
  }
}

function mediaSrc(att: RenderableAttachment): string | null {
  if (isUsableAttachmentPreviewUrl(att.previewUrl)) return att.previewUrl!.trim()
  return loadedPreviews.value[att.id] ?? null
}

function filePath(att: RenderableAttachment): string | undefined {
  return resolvedAbsPaths.value[att.id] ?? att.localAbsPath
}

async function copyFilePath(att: RenderableAttachment) {
  const path = filePath(att) ?? (await resolveAttachmentAbsPath(att))
  if (!path) return
  try {
    await navigator.clipboard.writeText(path)
  } catch (e) {
    console.warn('copy file path failed', e)
  }
}

async function onRevealInFinder(att: RenderableAttachment) {
  const path = filePath(att) ?? (await resolveAttachmentAbsPath(att))
  if (!path) return
  try {
    await revealInFinder(path)
  } catch (e) {
    console.warn('reveal in finder failed', e)
  }
}

async function onOpenAttachment(att: RenderableAttachment) {
  try {
    await openAttachmentWithSystemDefault(att)
  } catch (e) {
    console.warn('open attachment failed', e)
  }
}

const alignClass = computed(() =>
  props.align === 'end' ? 'items-end' : 'items-start'
)

watch(
  () => props.attachments,
  list => {
    for (const att of list) void ensureMediaPreview(att)
    void prefetchAttachmentAbsPaths(list).then(map => {
      resolvedAbsPaths.value = map
    })
  },
  { immediate: true, deep: true }
)
</script>

<template>
  <div
    v-if="attachments.length"
    class="flex flex-col gap-2 w-full"
    :class="alignClass"
  >
    <template v-for="att in attachments" :key="att.id">
      <div class="relative group/media-attachment">
        <img
          v-if="att.kind === 'image' && mediaSrc(att)"
          :src="mediaSrc(att)!"
          alt=""
          class="max-h-64 max-w-full rounded-xl border border-border object-contain"
        />
        <div
          v-else-if="att.kind === 'audio'"
          class="flex flex-col gap-1 max-w-sm"
          :class="align === 'end' ? 'items-end' : 'items-start'"
        >
          <ChatAudioPlayer
            :variant="align === 'end' ? 'user' : 'default'"
            :src="mediaSrc(att)"
          />
          <ChatAudioTranscript
            v-if="att.derivedText?.trim()"
            :text="att.derivedText.trim()"
            :align="align === 'end' ? 'end' : 'start'"
          />
        </div>
        <video
          v-else-if="att.kind === 'video' && mediaSrc(att)"
          controls
          preload="metadata"
          class="max-h-64 max-w-full rounded-xl border border-border"
          :src="mediaSrc(att)!"
        />
        <button
          v-else-if="isOpenableFileAttachment(att.kind)"
          type="button"
          class="inline-flex items-center gap-2 rounded-xl border border-border bg-muted/30 px-3 py-2 text-xs text-foreground cursor-pointer transition-colors hover:bg-muted/50"
          :title="`打开 ${att.fileName}`"
          @click="onOpenAttachment(att)"
        >
          <FileText class="h-4 w-4 shrink-0 text-muted" />
          <span class="truncate max-w-[240px]">{{ att.fileName }}</span>
        </button>
        <div
          v-else
          class="inline-flex items-center gap-2 rounded-xl border border-border bg-muted/30 px-3 py-2 text-xs text-foreground"
        >
          <FileText class="h-4 w-4 shrink-0 text-muted" />
          <span class="truncate max-w-[240px]" :title="att.fileName">{{ att.fileName }}</span>
        </div>
        <!-- 操作按钮悬浮层 -->
        <div
          v-if="(filePath(att) || att.storageRelPath) && (att.kind === 'image' || att.kind === 'video')"
          class="absolute top-2 right-2 hidden group-hover/media-attachment:flex gap-1 bg-background/80 backdrop-blur-sm rounded-lg p-1 shadow border border-border"
        >
          <button
            class="p-1 rounded-md hover:bg-muted transition-colors"
            title="复制文件路径"
            @click="copyFilePath(att)"
          >
            <Clipboard class="h-3.5 w-3.5" />
          </button>
          <button
            v-if="isTauriRuntime()"
            class="p-1 rounded-md hover:bg-muted transition-colors"
            title="在 Finder 中显示"
            @click="onRevealInFinder(att)"
          >
            <FolderOpen class="h-3.5 w-3.5" />
          </button>
        </div>
      </div>
    </template>
  </div>
</template>
