<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue'
import MessageBubble from './MessageBubble.vue'
import { useChatStore } from '../../stores/chat'

const chat = useChatStore()
const scroller = ref<HTMLDivElement | null>(null)

async function toBottom() {
  await nextTick()
  const el = scroller.value
  if (el) el.scrollTop = el.scrollHeight
}

onMounted(toBottom)
watch(() => chat.current?.messages.length, toBottom)
watch(
  () => chat.current?.messages.map(m => m.content + (m.toolCalls?.length || 0)).join('|'),
  toBottom
)
</script>

<template>
  <div ref="scroller" class="h-full overflow-y-auto px-6 md:px-10 pb-6">
    <div class="max-w-3xl mx-auto pt-6 space-y-5">
      <MessageBubble
        v-for="m in chat.current?.messages || []"
        :key="m.id"
        :message="m"
      />
    </div>
  </div>
</template>
