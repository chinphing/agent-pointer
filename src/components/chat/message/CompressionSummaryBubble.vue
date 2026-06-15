<script setup lang="ts">
import { computed, ref } from 'vue'
import { parseMarkdown } from '../../../lib/markdownConfig'
import { Archive, ChevronDown, ChevronRight } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import { compressionSummaryBody } from '../../../lib/compressionMessage'
import { useMarkdownCodeCopy } from '../../../composables/useMarkdownCodeCopy'
import { useMarkdownExternalLinks } from '../../../composables/useMarkdownExternalLinks'
import MessageFooterActions from './MessageFooterActions.vue'

const props = defineProps<{ message: ChatMessage }>()

const expanded = ref(false)
const bodyRef = ref<HTMLElement | null>(null)

const summaryBody = computed(() => compressionSummaryBody(props.message.content))

const html = computed(() => parseMarkdown(summaryBody.value))

useMarkdownCodeCopy(bodyRef, () => summaryBody.value)
useMarkdownExternalLinks(bodyRef, () => summaryBody.value)
</script>

<template>
  <div class="chat-hover-root relative chat-column">
    <div
      class="message-avatar-slot absolute right-full mr-2 top-0 w-8 h-8 rounded-lg shrink-0 flex items-center justify-center bg-accent-muted/30 border border-border"
    >
      <Archive class="w-4 h-4 text-muted" aria-hidden="true" />
    </div>

    <button
      type="button"
      class="w-full text-left rounded-xl border border-border bg-card px-3 py-2.5 transition-colors hover:bg-hover/50"
      @click="expanded = !expanded"
    >
      <div class="flex items-center gap-2 min-w-0">
        <component
          :is="expanded ? ChevronDown : ChevronRight"
          class="w-3.5 h-3.5 shrink-0 text-muted"
          aria-hidden="true"
        />
        <span class="text-[12px] font-medium text-foreground">较早对话摘要</span>
        <span class="text-[10px] text-muted">自动压缩</span>
      </div>
      <p v-if="!expanded" class="mt-1.5 text-[11px] text-muted line-clamp-2 pl-5">
        {{ summaryBody }}
      </p>
    </button>

    <div
      v-if="expanded"
      class="mt-1.5 rounded-xl border border-border bg-accent-muted/20 px-3 py-3"
    >
      <div ref="bodyRef" class="md-body text-[13px] text-foreground" v-html="html" />
    </div>

    <MessageFooterActions
      :created-at="message.createdAt"
      :copy-text="summaryBody"
      :show-copy="!!summaryBody?.trim()"
    />
  </div>
</template>
