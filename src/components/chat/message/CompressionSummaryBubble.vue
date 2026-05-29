<script setup lang="ts">
import { computed, ref } from 'vue'
import { marked } from 'marked'
import { Archive, ChevronDown, ChevronRight } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import { compressionSummaryBody } from '../../../lib/compressionMessage'
import { useMarkdownCodeCopy } from '../../../composables/useMarkdownCodeCopy'
import { useMarkdownExternalLinks } from '../../../composables/useMarkdownExternalLinks'
import MessageTimeChip from './MessageTimeChip.vue'

const props = defineProps<{ message: ChatMessage }>()

const expanded = ref(false)
const bodyRef = ref<HTMLElement | null>(null)

marked.setOptions({ breaks: true, gfm: true })

const summaryBody = computed(() => compressionSummaryBody(props.message.content))

const html = computed(() =>
  summaryBody.value ? (marked.parse(summaryBody.value) as string) : ''
)

useMarkdownCodeCopy(bodyRef, () => summaryBody.value)
useMarkdownExternalLinks(bodyRef, () => summaryBody.value)
</script>

<template>
  <div class="flex gap-3">
    <div
      class="w-8 h-8 rounded-lg shrink-0 flex items-center justify-center bg-accent-muted/30 border border-border"
    >
      <Archive class="w-4 h-4 text-muted" aria-hidden="true" />
    </div>

    <div class="flex-1 min-w-0 max-w-[88%]">
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
          <MessageTimeChip :created-at="message.createdAt" class="ml-auto shrink-0" />
        </div>
        <p v-if="!expanded" class="mt-1.5 text-[11px] text-muted line-clamp-2 pl-5">
          {{ summaryBody }}
        </p>
      </button>

      <div
        v-if="expanded"
        class="mt-1.5 rounded-xl border border-border bg-accent-muted/20 px-4 py-3"
      >
        <div ref="bodyRef" class="md-body text-[13px] text-foreground" v-html="html" />
      </div>
    </div>
  </div>
</template>
