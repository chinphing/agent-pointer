<script setup lang="ts">
import { AlertCircle } from 'lucide-vue-next'
import type { ChatMessage } from '../../../../types/chat'
import MessageFooterActions from '../MessageFooterActions.vue'

defineProps<{ message: ChatMessage }>()
</script>

<template>
  <div
    class="w-full rounded-2xl border border-danger/40 bg-danger/10 px-3 py-3 text-[14px] leading-relaxed text-foreground"
    role="alert"
  >
    <div class="flex gap-2 items-start min-w-0">
      <AlertCircle class="w-4 h-4 shrink-0 mt-0.5 text-danger" />
      <div class="min-w-0 flex-1">
        <div class="text-[11px] uppercase tracking-wide text-danger/90 mb-1">助手消息异常</div>
        <div class="whitespace-pre-wrap break-words">{{ message.errorMessage || '生成失败' }}</div>
      </div>
    </div>
    <MessageFooterActions
      :created-at="message.createdAt"
      :copy-text="message.errorMessage || '生成失败'"
      :show-copy="true"
    />
  </div>
</template>
