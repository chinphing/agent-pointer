<script setup lang="ts">
import { computed, ref } from 'vue'
import { parseMarkdown } from '../../../lib/markdownConfig'
import type { ChatMessage } from '../../../types/chat'
import { compressionSummaryBody } from '../../../lib/compressionMessage'
import { useMarkdownCodeCopy } from '../../../composables/useMarkdownCodeCopy'
import { useMarkdownExternalLinks } from '../../../composables/useMarkdownExternalLinks'

const props = defineProps<{ message: ChatMessage }>()

const expanded = ref(false)
const bodyRef = ref<HTMLElement | null>(null)

const summaryBody = computed(() => compressionSummaryBody(props.message.content))

const html = computed(() => parseMarkdown(summaryBody.value))

useMarkdownCodeCopy(bodyRef, () => summaryBody.value)
useMarkdownExternalLinks(bodyRef, () => summaryBody.value)
</script>

<template>
  <div class="chat-column px-3" role="group">
    <button
      type="button"
      class="py-0.5 inline-flex items-center gap-0.5 text-[11px] text-muted hover:text-foreground/70 transition-colors cursor-pointer"
      :aria-expanded="expanded"
      @click="expanded = !expanded"
    >
      <span>自动压缩摘要</span>
      <span class="select-none" aria-hidden="true">{{ expanded ? '∨' : '>' }}</span>
    </button>

    <div
      v-if="expanded && summaryBody.trim()"
      ref="bodyRef"
      class="mt-1 md-body text-[11px] text-muted max-w-none"
      v-html="html"
    />
  </div>
</template>
