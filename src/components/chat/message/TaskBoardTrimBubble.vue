<script setup lang="ts">
import { computed, ref } from 'vue'
import { ClipboardList, ChevronDown, ChevronRight } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import { taskBoardTrimNoticeBody } from '../../../lib/taskBoardTrimMessage'
import MessageTimeChip from './MessageTimeChip.vue'

const props = defineProps<{ message: ChatMessage }>()

const expanded = ref(false)

const body = computed(() => taskBoardTrimNoticeBody(props.message.content))
</script>

<template>
  <div class="flex gap-3">
    <div
      class="w-8 h-8 rounded-lg shrink-0 flex items-center justify-center bg-sky-500/15 border border-sky-500/25"
    >
      <ClipboardList class="w-4 h-4 text-sky-300" aria-hidden="true" />
    </div>

    <div class="flex-1 min-w-0 max-w-[88%]">
      <button
        type="button"
        class="w-full text-left rounded-xl border border-sky-500/20 bg-sky-500/[0.06] px-3 py-2.5 transition-colors hover:bg-sky-500/[0.09]"
        @click="expanded = !expanded"
      >
        <div class="flex items-center gap-2 min-w-0">
          <component
            :is="expanded ? ChevronDown : ChevronRight"
            class="w-3.5 h-3.5 shrink-0 text-sky-300/80"
            aria-hidden="true"
          />
          <span class="text-[12px] font-medium text-sky-100/90">任务板更新后精简历史</span>
          <span class="text-[10px] text-sky-200/50">checkpoint</span>
          <MessageTimeChip :created-at="message.createdAt" class="ml-auto shrink-0" />
        </div>
        <p v-if="!expanded" class="mt-1.5 text-[11px] text-slate-400 line-clamp-2 pl-5">
          {{ body }}
        </p>
      </button>

      <div
        v-if="expanded"
        class="mt-1.5 rounded-xl border border-sky-500/15 bg-black/20 px-4 py-3 text-[13px] text-slate-300 leading-relaxed"
      >
        {{ body }}
      </div>
    </div>
  </div>
</template>
