<script setup lang="ts">
import { AlertCircle } from 'lucide-vue-next'
import type { ChatMessage } from '../../../../types/chat'

defineProps<{ message: ChatMessage }>()
</script>

<template>
  <div
    class="rounded-2xl border border-danger/40 bg-danger/10 px-4 py-3 text-[14px] leading-relaxed text-slate-100 max-w-full"
    role="alert"
  >
    <div class="flex gap-2 items-start">
      <AlertCircle class="w-4 h-4 shrink-0 mt-0.5 text-danger" />
      <div class="min-w-0">
        <div class="text-[11px] uppercase tracking-wide text-danger/90 mb-1">助手消息异常</div>
        <div class="whitespace-pre-wrap break-words">{{ message.errorMessage || '生成失败' }}</div>
      </div>
    </div>
  </div>
</template>
