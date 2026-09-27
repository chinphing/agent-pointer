<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { Ban } from 'lucide-vue-next'
import type { ChatMessage } from '../../../../types/chat'


const { t } = useI18n()
defineProps<{ message: ChatMessage }>()
</script>

<template>
  <div class="tool-call-row px-3" role="status">
    <div class="py-0.5 inline-flex items-center gap-1 text-[11px] text-muted">
      <Ban class="w-3 h-3 text-muted/70 shrink-0" aria-hidden="true" />
      <span>{{ t('chat.message.generationStopped') }}</span>
    </div>
  </div>
</template>
