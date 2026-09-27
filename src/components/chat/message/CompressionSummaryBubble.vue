<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ChevronDown, ChevronRight } from 'lucide-vue-next'
import { parseMarkdown } from '../../../lib/markdownConfig'
import type { ChatMessage } from '../../../types/chat'
import { compressionSummaryBody } from '../../../lib/compressionMessage'
import { useMarkdownCodeCopy } from '../../../composables/useMarkdownCodeCopy'
import { useMarkdownCharts } from '../../../composables/useMarkdownCharts'
import { useMarkdownSvgs } from '../../../composables/useMarkdownSvgs'
import { useMarkdownMermaid } from '../../../composables/useMarkdownMermaid'
import { useMarkdownExternalLinks } from '../../../composables/useMarkdownExternalLinks'


const { t } = useI18n()
const props = defineProps<{ message: ChatMessage }>()

const expanded = ref(false)
const bodyRef = ref<HTMLElement | null>(null)

const summaryBody = computed(() => compressionSummaryBody(props.message.content))

const html = computed(() => parseMarkdown(summaryBody.value))

useMarkdownCodeCopy(bodyRef, () => html.value)
useMarkdownCharts(bodyRef, () => html.value)
useMarkdownSvgs(bodyRef, () => html.value)
useMarkdownMermaid(bodyRef, () => html.value)
useMarkdownExternalLinks(bodyRef, () => html.value)
</script>

<template>
  <div class="chat-column px-3" role="group">
    <button
      type="button"
      class="tool-call-trigger py-0.5 inline-flex items-center gap-0.5 text-[11px] text-muted hover:text-foreground/70 transition-colors cursor-pointer"
      :aria-expanded="expanded"
      @click="expanded = !expanded"
    >
      <span>{{ t('chat.message.compressionSummary') }}</span>
      <component
        :is="expanded ? ChevronDown : ChevronRight"
        class="tool-call-chevron h-3 w-3 shrink-0 hidden"
        aria-hidden="true"
      />
    </button>

    <div
      v-if="expanded && summaryBody.trim()"
      ref="bodyRef"
      class="mt-1 md-body text-[11px] text-muted max-w-none"
      v-html="html"
    />
  </div>
</template>
