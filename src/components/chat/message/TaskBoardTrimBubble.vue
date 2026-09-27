<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ClipboardList, ChevronDown, ChevronRight } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import { taskBoardTrimNoticeBody } from '../../../lib/taskBoardTrimMessage'
import MessageFooterActions from './MessageFooterActions.vue'


const { t } = useI18n()
const props = defineProps<{ message: ChatMessage }>()

const expanded = ref(false)

const body = computed(() => taskBoardTrimNoticeBody(props.message.content))
</script>

<template>
  <div class="chat-hover-root relative chat-column">
    <div
      class="message-avatar-slot absolute right-full mr-2 top-0 w-8 h-8 rounded-lg shrink-0 flex items-center justify-center bg-accent-muted/30 border border-border"
    >
      <ClipboardList class="w-4 h-4 text-muted" aria-hidden="true" />
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
        <span class="text-[12px] font-medium text-foreground">{{ t('chat.message.taskBoardTrim') }}</span>
        <span class="text-[10px] text-muted">{{ t('chat.message.legacyPlaceholder') }}</span>
      </div>
      <p v-if="!expanded" class="mt-1.5 text-[11px] text-muted line-clamp-2 pl-5">
        {{ body }}
      </p>
    </button>

    <div
      v-if="expanded"
      class="mt-1.5 rounded-xl border border-border bg-accent-muted/20 px-3 py-3 text-[13px] text-foreground leading-relaxed"
    >
      {{ body }}
    </div>

    <MessageFooterActions
      :created-at="message.createdAt"
      :copy-text="body"
      :show-copy="!!body?.trim()"
    />
  </div>
</template>
