<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue'
import { ArrowDown } from 'lucide-vue-next'
import MessageBubble from './MessageBubble.vue'
import { useChatStore } from '../../stores/chat'

const chat = useChatStore()
const scroller = ref<HTMLDivElement | null>(null)
const showScrollButton = ref(false)

async function toBottom() {
  await nextTick()
  const el = scroller.value
  if (el) el.scrollTop = el.scrollHeight
}

function onScroll() {
  const el = scroller.value
  if (!el) return
  const isNearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 100
  showScrollButton.value = !isNearBottom
}

onMounted(toBottom)
watch(() => chat.current?.messages.length, toBottom)
watch(
  () => chat.current?.messages.map(m => m.content + (m.toolCalls?.length || 0)).join('|'),
  toBottom
)
</script>

<template>
  <div ref="scroller" class="h-full overflow-y-auto px-6 md:px-10 pb-6" @scroll="onScroll">
    <div class="max-w-3xl mx-auto pt-6 space-y-5">
      <MessageBubble
        v-for="m in chat.current?.messages || []"
        :key="m.id"
        :message="m"
      />
    </div>

    <button
      v-if="showScrollButton"
      class="fixed bottom-32 right-8 h-10 w-10 rounded-full glass-strong border border-white/10 shadow-lg flex items-center justify-center cursor-pointer hover:bg-white/10 transition"
      @click="toBottom"
      title="滚动到底部"
    >
      <ArrowDown class="w-5 h-5 text-slate-200" />
    </button>
  </div>
</template>
