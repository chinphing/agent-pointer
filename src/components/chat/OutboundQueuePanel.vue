<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ArrowUp, ChevronDown, Clock, Paperclip, X } from 'lucide-vue-next'
import type { OutboundQueueItem } from '../../types/chat'
import { useChatStore } from '../../stores/chat'

const props = defineProps<{
  items: OutboundQueueItem[]
  conversationId: string
}>()

const chat = useChatStore()
const expanded = ref(false)
const forcingId = ref<string | null>(null)

const count = computed(() => props.items.length)

watch(
  () => count.value,
  (n, prev) => {
    if (n > 0 && (prev ?? 0) === 0) expanded.value = true
    if (n === 0) expanded.value = false
  },
  { immediate: true }
)

function preview(item: OutboundQueueItem): string {
  const text = item.content.trim()
  if (text) return text.length > 120 ? `${text.slice(0, 119)}…` : text
  const n = item.attachments?.length ?? 0
  if (n > 0) return `${n} 个附件`
  return '（空消息）'
}

function removeItem(itemId: string) {
  chat.removeOutboundQueueItem(props.conversationId, itemId)
}

async function forceSend(itemId: string) {
  if (forcingId.value) return
  forcingId.value = itemId
  try {
    await chat.forceSendOutbound(props.conversationId, itemId)
  } finally {
    forcingId.value = null
  }
}
</script>

<template>
  <div
    v-if="count > 0"
    class="mb-2 rounded-xl border border-border panel-elevated overflow-hidden shadow-sm"
  >
    <button
      type="button"
      class="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-hover transition-colors cursor-pointer"
      :aria-expanded="expanded"
      @click="expanded = !expanded"
    >
      <Clock class="w-3.5 h-3.5 text-accent shrink-0" aria-hidden="true" />
      <span class="text-[12px] font-medium text-foreground flex-1 min-w-0 truncate">
        待发送
      </span>
      <span class="text-[10px] px-1.5 py-0.5 rounded bg-accent-muted text-accent shrink-0 tabular-nums">
        {{ count }}
      </span>
      <span class="hidden sm:inline text-[10px] text-muted shrink-0">
        Enter 立即发送队首 · ⌘/Ctrl+Enter 停止并发送
      </span>
      <ChevronDown
        class="w-3.5 h-3.5 text-muted shrink-0 transition-transform duration-200"
        :class="expanded ? 'rotate-180' : ''"
        aria-hidden="true"
      />
    </button>

    <ul
      v-show="expanded"
      class="border-t border-border divide-y divide-border bg-accent-muted/10 max-h-48 overflow-y-auto"
    >
      <li
        v-for="(item, index) in items"
        :key="item.id"
        class="flex items-start gap-2 px-3 py-2 text-[12px] hover:bg-hover/60 transition-colors"
      >
        <span
          class="text-[10px] tabular-nums shrink-0 pt-0.5 min-w-[1.25rem] text-center rounded bg-hover text-muted"
        >
          {{ index + 1 }}
        </span>
        <div class="min-w-0 flex-1">
          <p class="text-foreground whitespace-pre-wrap break-words leading-snug">{{ preview(item) }}</p>
          <p v-if="item.attachments?.length" class="mt-1 flex items-center gap-1 text-[10px] text-muted">
            <Paperclip class="w-3 h-3 shrink-0" aria-hidden="true" />
            {{ item.attachments.length }} 个附件
          </p>
        </div>
        <div class="shrink-0 flex items-center gap-0.5 pt-0.5">
          <button
            type="button"
            class="inline-flex items-center justify-center h-7 w-7 rounded-md text-muted hover:text-accent hover:bg-hover cursor-pointer transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            title="强制发送：暂停当前任务并立即发送（⌘/Ctrl+Enter）"
            aria-label="强制发送"
            :disabled="forcingId !== null"
            @click.stop="forceSend(item.id)"
          >
            <ArrowUp class="w-4 h-4" stroke-width="2.25" />
          </button>
          <button
            type="button"
            class="inline-flex items-center justify-center h-7 w-7 rounded-md text-muted hover:text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            title="移出队列"
            aria-label="移出队列"
            :disabled="forcingId !== null"
            @click.stop="removeItem(item.id)"
          >
            <X class="w-3.5 h-3.5" />
          </button>
        </div>
      </li>
    </ul>
  </div>
</template>
