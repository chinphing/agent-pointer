<script setup lang="ts">
import { computed, ref } from 'vue'
import { marked } from 'marked'
import { User, Copy, Check } from 'lucide-vue-next'
import type { ChatMessage } from '../../../types/chat'
import { useMarkdownCodeCopy } from '../../../composables/useMarkdownCodeCopy'

const props = defineProps<{ message: ChatMessage }>()

const bodyRef = ref<HTMLElement | null>(null)
const copied = ref(false)

marked.setOptions({ breaks: true, gfm: true })

const html = computed(() =>
  props.message.content ? (marked.parse(props.message.content) as string) : ''
)

useMarkdownCodeCopy(bodyRef, () => props.message.content)

function copy() {
  void navigator.clipboard.writeText(props.message.content).then(() => {
    copied.value = true
    setTimeout(() => {
      copied.value = false
    }, 2000)
  })
}
</script>

<template>
  <div class="flex gap-3 flex-row-reverse">
    <div
      class="w-8 h-8 rounded-lg shrink-0 flex items-center justify-center bg-gradient-to-br from-slate-600 to-slate-700"
    >
      <User class="w-4 h-4 text-white" />
    </div>

    <div class="flex-1 min-w-0 flex flex-col items-end">
      <div
        class="block overflow-hidden px-4 py-3 rounded-2xl border break-words w-fit max-w-[80%] bg-primary/15 border-primary/25 text-slate-100"
      >
        <div v-if="message.content" ref="bodyRef" class="md-body" v-html="html" />
      </div>
      <div class="mt-1.5 flex items-center gap-1 justify-end w-full max-w-[80%]">
        <button
          class="p-1.5 rounded hover:bg-white/5 cursor-pointer transition"
          :class="copied ? 'text-green-400' : 'text-slate-400 hover:text-slate-200'"
          :title="copied ? '已复制' : '复制'"
          @click="copy"
        >
          <Check v-if="copied" class="w-3.5 h-3.5" />
          <Copy v-else class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>
  </div>
</template>
