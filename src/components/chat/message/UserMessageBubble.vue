<script setup lang="ts">
import { computed, ref } from 'vue'
import { marked } from 'marked'
import { User } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import { isContextExcluded } from '../../../lib/messageContext'
import ContextExcludedFooter from './ContextExcludedFooter.vue'
import MessageFooterActions from './MessageFooterActions.vue'
import { useMarkdownCodeCopy } from '../../../composables/useMarkdownCodeCopy'
import { useMarkdownExternalLinks } from '../../../composables/useMarkdownExternalLinks'

const props = defineProps<{ message: ChatMessage }>()

const bodyRef = ref<HTMLElement | null>(null)

marked.setOptions({ breaks: true, gfm: true })

const html = computed(() =>
  props.message.content ? (marked.parse(props.message.content) as string) : ''
)

useMarkdownCodeCopy(bodyRef, () => props.message.content)
useMarkdownExternalLinks(bodyRef, () => props.message.content)
</script>

<template>
  <div class="chat-hover-root relative chat-column flex justify-end">
    <div
      class="message-avatar-slot absolute left-full ml-2 top-0 w-8 h-8 rounded-lg shrink-0 flex items-center justify-center bg-gradient-to-br from-slate-600 to-slate-700"
    >
      <User class="w-4 h-4 text-white" />
    </div>

    <div class="max-w-[85%] min-w-0 flex flex-col items-end">
      <div
        class="relative w-full rounded-2xl px-3 py-2.5 panel-elevated break-words text-foreground"
        :class="isContextExcluded(message) ? 'opacity-80' : ''"
      >
        <div
          v-if="message.content"
          ref="bodyRef"
          class="md-body md-body-flow"
          v-html="html"
        />
        <ContextExcludedFooter :message="message" />
        <MessageFooterActions
          v-if="message.content"
          class="message-footer-actions--inset"
          :created-at="message.createdAt"
          :copy-text="message.content"
          :show-copy="true"
        />
      </div>
    </div>
  </div>
</template>
