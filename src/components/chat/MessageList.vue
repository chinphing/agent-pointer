<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { ArrowDown } from 'lucide-vue-next'
import MessageRow from './message/MessageRow.vue'
import TaskBoardPanel from './TaskBoardPanel.vue'
import { useChatStore } from '../../stores/chat'
import type { ChatMessage } from '../../types/chat'

const chat = useChatStore()
const scroller = ref<HTMLDivElement | null>(null)
const showScrollButton = ref(false)

async function toBottom() {
  await nextTick()
  const el = scroller.value
  if (el) el.scrollTop = el.scrollHeight
}

function isNearBottom(): boolean {
  const el = scroller.value
  if (!el) return true
  return el.scrollHeight - el.scrollTop - el.clientHeight < 100
}

function onScroll() {
  showScrollButton.value = !isNearBottom()
}

onMounted(toBottom)

// 只有当用户已经在底部附近时，新消息才自动滚动到底部
watch(() => chat.current?.messages.length, () => {
  if (isNearBottom()) {
    toBottom()
  }
})
watch(
  () => chat.current?.messages.map(m => m.content + (m.toolCalls?.length || 0)).join('|'),
  () => {
    if (isNearBottom()) {
      toBottom()
    }
  }
)

type FlatEntry = { type: 'message'; message: ChatMessage } | { type: 'task_board' }

const flatMessages = computed<FlatEntry[]>(() => {
  const msgs = chat.current?.messages ?? []
  if (!chat.currentId) return msgs.map(m => ({ type: 'message' as const, message: m }))
  const anchorId = chat.taskBoardAnchorMessageIds.get(chat.currentId)
  const anchorIdx = anchorId ? msgs.findIndex(m => m.id === anchorId) : -1
  const entries: FlatEntry[] = []
  for (let i = 0; i < msgs.length; i++) {
    entries.push({ type: 'message', message: msgs[i] })
    if (i === anchorIdx) {
      entries.push({ type: 'task_board' })
    }
  }
  return entries
})
</script>

<template>
  <div ref="scroller" class="h-full overflow-y-auto px-6 md:px-10 pb-6" @scroll="onScroll">
    <div class="max-w-3xl mx-auto pt-6 space-y-5">
      <template v-for="entry in flatMessages" :key="entry.type === 'message' ? entry.message.id : 'task-board'">
        <MessageRow
          v-if="entry.type === 'message'"
          :message="entry.message"
        />
        <div
          v-else
          class="task-board-sticky sticky top-0 z-20 -mt-2 mb-1 flex justify-end pr-11 py-1 bg-background/95 backdrop-blur-sm"
        >
          <TaskBoardPanel
            v-if="chat.currentId"
            :document="(chat.taskBoardForConversation(chat.currentId)?.parent ?? null)"
            :child-boards="(chat.taskBoardForConversation(chat.currentId)?.children ?? undefined)"
          />
        </div>
      </template>
    </div>

    <button
      v-if="showScrollButton"
      class="fixed bottom-32 right-8 h-10 w-10 rounded-full panel shadow-lg flex items-center justify-center cursor-pointer hover:bg-hover transition"
      @click="toBottom"
      title="滚动到底部"
    >
      <ArrowDown class="w-5 h-5 text-foreground" />
    </button>
  </div>
</template>
