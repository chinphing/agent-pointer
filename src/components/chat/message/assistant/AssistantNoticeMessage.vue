<script setup lang="ts">
import { computed } from 'vue'
import { Info, Monitor } from 'lucide-vue-next'
import type { ChatMessage } from '../../../../types/chat'
import { injectedNoticeFlavor } from '../../../../lib/assistantMessageKind'
import MessageTimeChip from '../MessageTimeChip.vue'

const props = defineProps<{ message: ChatMessage }>()

const flavor = computed(() => injectedNoticeFlavor(props.message.content))

const shellClass = computed(() => {
  switch (flavor.value) {
    case 'desktop':
      return 'border-white/[0.06] bg-white/[0.02] text-slate-500'
    case 'hint':
      return 'border-white/[0.08] bg-white/[0.03] text-slate-500'
    default:
      return 'border-white/[0.08] bg-white/[0.03] text-slate-500'
  }
})

const icon = computed(() => (flavor.value === 'desktop' ? Monitor : Info))
</script>

<template>
  <div
    class="flex gap-1.5 items-center rounded-md border px-2 py-1 text-[11px] leading-normal max-w-full font-normal min-w-0"
    :class="shellClass"
    role="status"
  >
    <component :is="icon" class="w-3 h-3 shrink-0 opacity-60" aria-hidden="true" />
    <div class="min-w-0 flex-1 whitespace-pre-wrap break-words">{{ message.content }}</div>
    <MessageTimeChip :created-at="message.createdAt" class="shrink-0" />
  </div>
</template>
