<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { parseMarkdown } from '../../../lib/markdownConfig'
import { Clipboard, FileText, FolderOpen, User } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import type { RenderableAttachment } from '../../../lib/messageNormalizer'
import ChatAudioPlayer from './ChatAudioPlayer.vue'
import ChatAudioTranscript from './ChatAudioTranscript.vue'
import { userMessageDisplayContent } from '../../../lib/messageNormalizer'
import MessageFooterActions from './MessageFooterActions.vue'
import { useMarkdownCodeCopy } from '../../../composables/useMarkdownCodeCopy'
import { useMarkdownExternalLinks } from '../../../composables/useMarkdownExternalLinks'
import { isUsableAttachmentPreviewUrl } from '../../../lib/attachmentSupport'
import { attachmentsForMessageRender } from '../../../lib/messageNormalizer'
import { previewChatMedia, revealInFinder } from '../../../lib/api'
import { prefetchAttachmentAbsPaths, resolveAttachmentAbsPath } from '../../../lib/attachmentLocalPath'
import { resolveVideoPreviewUrl } from '../../../lib/chatMediaPreview'
import {
  isOpenableFileAttachment,
  openAttachmentWithSystemDefault
} from '../../../lib/openAttachment'
import { isTauriRuntime } from '../../../lib/runtime'

const props = defineProps<{ message: ChatMessage }>()

const bodyRef = ref<HTMLElement | null>(null)
const loadedPreviews = ref<Record<string, string>>({})
const resolvedAbsPaths = ref<Record<string, string>>({})
const previewInflight = new Set<string>()

const displayContent = computed(() => userMessageDisplayContent(props.message))

const html = computed(() => parseMarkdown(displayContent.value))

const attachments = computed(() => attachmentsForMessageRender(props.message))

useMarkdownCodeCopy(bodyRef, () => props.message.content)
useMarkdownExternalLinks(bodyRef, () => props.message.content)

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
    if (!att.storageRelPath) return
    const preview = await previewChatMedia(att.storageRelPath)
    const mime = preview.mimeType || 'application/octet-stream'
    loadedPreviews.value = {
      ...loadedPreviews.value,
      [att.id]: `data:${mime};base64,${preview.dataBase64}`
    }
  } catch (e) {
    console.warn('previewChatMedia failed', e)
  } finally {
    previewInflight.delete(att.id)
  }
}

function mediaSrc(att: { id: string; previewUrl?: string }): string | null {
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

watch(
  attachments,
  list => {
    for (const att of list) {
      void ensureMediaPreview(att)
    }
    void prefetchAttachmentAbsPaths(list).then(map => {
      resolvedAbsPaths.value = map
    })
  },
  { immediate: true }
)
</script>

<template>
  <div class="chat-hover-root relative chat-column flex justify-end">
    <div
      class="message-avatar-slot absolute left-full ml-2 top-0 w-8 h-8 rounded-lg shrink-0 flex items-center justify-center bg-gradient-to-br from-slate-600 to-slate-700"
    >
      <User class="w-4 h-4 text-white" />
    </div>

    <div class="max-w-[85%] min-w-0 flex flex-col items-end gap-2">
      <div
        v-if="attachments.length"
        class="flex flex-col items-end gap-2 w-full"
      >
        <template v-for="att in attachments" :key="att.id">
          <div class="relative group/media-attachment">
            <img
              v-if="att.kind === 'image' && mediaSrc(att)"
              :src="mediaSrc(att)!"
              :alt="att.fileName"
              class="max-h-64 max-w-full rounded-xl border border-border object-contain"
            />
            <div
              v-else-if="att.kind === 'audio'"
              class="flex flex-col gap-1 items-end max-w-sm"
            >
              <ChatAudioPlayer
                variant="user"
                :src="mediaSrc(att)"
              />
              <ChatAudioTranscript
                v-if="att.derivedText?.trim()"
                :text="att.derivedText.trim()"
                align="end"
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

      <div
        v-if="displayContent"
        class="relative w-full rounded-2xl px-3 pt-2 pb-2 panel-elevated break-words text-foreground"
      >
        <div
          ref="bodyRef"
          class="md-body md-body-flow"
          v-html="html"
        />
        <MessageFooterActions
          class="justify-end"
          :created-at="message.createdAt"
          :copy-text="displayContent"
          :show-copy="true"
        />
      </div>
    </div>
  </div>
</template>
