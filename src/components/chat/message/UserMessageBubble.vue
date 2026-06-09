<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { marked } from 'marked'
import { Clipboard, FileText, FolderOpen, User } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import type { RenderableAttachment } from '../../../lib/messageNormalizer'
import MessageFooterActions from './MessageFooterActions.vue'
import { useMarkdownCodeCopy } from '../../../composables/useMarkdownCodeCopy'
import { useMarkdownExternalLinks } from '../../../composables/useMarkdownExternalLinks'
import { attachmentsForMessageRender } from '../../../lib/messageNormalizer'
import { previewChatMedia, revealInFinder } from '../../../lib/api'
import { isTauriRuntime } from '../../../lib/runtime'

const props = defineProps<{ message: ChatMessage }>()

const bodyRef = ref<HTMLElement | null>(null)
const loadedPreviews = ref<Record<string, string>>({})

marked.setOptions({ breaks: true, gfm: true })

const html = computed(() =>
  props.message.content ? (marked.parse(props.message.content) as string) : ''
)

const attachments = computed(() => attachmentsForMessageRender(props.message))

useMarkdownCodeCopy(bodyRef, () => props.message.content)
useMarkdownExternalLinks(bodyRef, () => props.message.content)

async function ensureMediaPreview(attId: string, storageRelPath?: string) {
  if (!storageRelPath || loadedPreviews.value[attId]) return
  try {
    const preview = await previewChatMedia(storageRelPath)
    const mime = preview.mimeType || 'application/octet-stream'
    loadedPreviews.value = {
      ...loadedPreviews.value,
      [attId]: `data:${mime};base64,${preview.dataBase64}`
    }
  } catch (e) {
    console.warn('previewChatMedia failed', e)
  }
}

function mediaSrc(att: { id: string; previewUrl?: string; storageRelPath?: string }): string | null {
  if (att.previewUrl) return att.previewUrl
  return loadedPreviews.value[att.id] ?? null
}

function filePath(att: RenderableAttachment): string | undefined {
  return att.localAbsPath ?? att.mediaRef
}

async function copyFilePath(att: RenderableAttachment) {
  const path = filePath(att)
  if (!path) return
  try {
    await navigator.clipboard.writeText(path)
  } catch (e) {
    console.warn('copy file path failed', e)
  }
}

async function onRevealInFinder(att: RenderableAttachment) {
  const path = filePath(att)
  if (!path) return
  try {
    await revealInFinder(path)
  } catch (e) {
    console.warn('reveal in finder failed', e)
  }
}

watch(
  attachments,
  list => {
    for (const att of list) {
      if (att.storageRelPath && !att.previewUrl) {
        void ensureMediaPreview(att.id, att.storageRelPath)
      }
    }
  },
  { immediate: true }
)

onMounted(() => {
  for (const att of attachments.value) {
    if (att.storageRelPath && !att.previewUrl) {
      void ensureMediaPreview(att.id, att.storageRelPath)
    }
  }
})
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
            <!-- 操作按钮悬浮层 -->
            <div
              v-if="filePath(att) && (att.kind === 'image' || att.kind === 'video')"
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
        v-if="message.content"
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
          :copy-text="message.content"
          :show-copy="true"
        />
      </div>
    </div>
  </div>
</template>
