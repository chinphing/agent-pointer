<script setup lang="ts">
import { computed, ref } from 'vue'
import { parseMarkdown } from '../../../lib/markdownConfig'
import { Clipboard, Download, FileText, FolderOpen, User } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import type { RenderableAttachment } from '../../../lib/messageNormalizer'
import ChatAudioPlayer from './ChatAudioPlayer.vue'
import ChatAudioTranscript from './ChatAudioTranscript.vue'
import ZoomableImage from './ZoomableImage.vue'
import { userMessageDisplayContent } from '../../../lib/messageNormalizer'
import MessageFooterActions from './MessageFooterActions.vue'
import { useMarkdownCodeCopy } from '../../../composables/useMarkdownCodeCopy'
import { useMarkdownCharts } from '../../../composables/useMarkdownCharts'
import { useMarkdownSvgs } from '../../../composables/useMarkdownSvgs'
import { useMarkdownMermaid } from '../../../composables/useMarkdownMermaid'
import { useMarkdownExternalLinks } from '../../../composables/useMarkdownExternalLinks'
import { attachmentsForMessageRender } from '../../../lib/messageNormalizer'
import { showsWebDownloadOnly } from '../../../lib/chatAttachmentLoad'
import { useChatAttachmentDisplay } from '../../../composables/useChatAttachmentDisplay'
import {
  isOpenableFileAttachment,
  openAttachmentWithSystemDefault
} from '../../../lib/openAttachment'
import { isTauriRuntime } from '../../../lib/runtime'

const props = defineProps<{ message: ChatMessage }>()

const bodyRef = ref<HTMLElement | null>(null)

const displayContent = computed(() => userMessageDisplayContent(props.message))

const html = computed(() => parseMarkdown(displayContent.value))

const attachments = computed(() => attachmentsForMessageRender(props.message))

const {
  mediaSrc,
  filePath,
  onDownloadAttachment,
  copyFilePath,
  onRevealInFinder
} = useChatAttachmentDisplay(attachments)

useMarkdownCodeCopy(bodyRef, () => html.value)
useMarkdownCharts(bodyRef, () => html.value)
useMarkdownSvgs(bodyRef, () => html.value)
useMarkdownMermaid(bodyRef, () => html.value)
useMarkdownExternalLinks(bodyRef, () => html.value)

async function onOpenAttachment(att: RenderableAttachment) {
  try {
    await openAttachmentWithSystemDefault(att, mediaSrc(att))
  } catch (e) {
    console.warn('open attachment failed', e)
  }
}
</script>

<template>
  <div class="chat-hover-root relative chat-column flex justify-end">
    <div class="relative max-w-[85%] min-w-0 w-fit flex flex-col items-end gap-2">
      <div
        class="message-avatar-slot absolute left-full ml-2 top-0 w-8 h-8 rounded-lg shrink-0 flex items-center justify-center bg-hover border border-border"
        title="用户"
      >
        <User class="w-4 h-4 text-muted" />
      </div>

      <div
        v-if="attachments.length"
        class="flex flex-col items-end gap-2 w-full"
      >
        <template v-for="att in attachments" :key="att.id">
          <div class="relative group/media-attachment">
            <ZoomableImage
              v-if="att.kind === 'image' && mediaSrc(att)"
              :src="mediaSrc(att)!"
              :alt="att.fileName"
            />
            <button
              v-else-if="showsWebDownloadOnly(att)"
              type="button"
              class="inline-flex max-w-full items-center gap-2 rounded-xl border border-border bg-muted/30 px-3 py-2 text-xs text-foreground cursor-pointer transition-colors hover:bg-muted/50"
              :title="`下载 ${att.fileName}`"
              @click="onDownloadAttachment(att)"
            >
              <FileText class="h-4 w-4 shrink-0 text-muted" />
              <span class="truncate max-w-[200px]">{{ att.fileName }}</span>
              <Download class="h-3.5 w-3.5 shrink-0 text-muted" />
              <span class="shrink-0 text-muted">下载</span>
            </button>
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
