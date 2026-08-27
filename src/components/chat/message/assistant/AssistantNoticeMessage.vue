<script setup lang="ts">
import { computed } from 'vue'
import { Info, Monitor, Archive } from 'lucide-vue-next'
import type { ChatMessage } from '../../../../types/chat'
import { injectedNoticeFlavor } from '../../../../lib/assistantMessageKind'
import MessageFooterActions from '../MessageFooterActions.vue'

const props = defineProps<{ message: ChatMessage }>()

const flavor = computed(() => injectedNoticeFlavor(props.message.content))

const shellClass = computed(() => {
  switch (flavor.value) {
    case 'hint':
      return 'border-border/60 bg-muted/10 text-muted'
    case 'compression':
      return 'border-warning/25 bg-warning/10 text-foreground'
    default:
      return 'border-border/60 bg-muted/10 text-muted'
  }
})

const isDesktopNotice = computed(() => flavor.value === 'desktop')

const icon = computed(() => {
  switch (flavor.value) {
    case 'desktop':
      return Monitor
    case 'compression':
      return Archive
    default:
      return Info
  }
})
</script>

<template>
  <div class="message-stamp-host w-full">
    <div
      v-if="isDesktopNotice"
      class="tool-call-row px-3"
      role="status"
    >
      <div
        class="py-1 flex flex-wrap items-center gap-x-1.5 gap-y-0.5 min-w-0 text-[11px] text-muted"
      >
        <Monitor class="w-3 h-3 text-muted/70 shrink-0" aria-hidden="true" />
        <span class="min-w-0 whitespace-pre-wrap break-words">{{ message.content }}</span>
      </div>
    </div>

    <div
      v-else
      class="flex gap-1.5 items-center min-w-0 text-[11px] leading-normal font-normal rounded-md border px-3 py-1.5"
      :class="shellClass"
      role="status"
    >
      <component :is="icon" class="w-3 h-3 shrink-0 opacity-60" aria-hidden="true" />
      <div class="min-w-0 flex-1 whitespace-pre-wrap break-words">{{ message.content }}</div>
    </div>

    <MessageFooterActions
      v-if="!isDesktopNotice"
      :created-at="message.createdAt"
      :copy-text="message.content"
      :show-copy="!!message.content?.trim()"
    />
  </div>
</template>
