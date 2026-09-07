<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { ChevronDown } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { collectLiveBackgroundJobs, type BackgroundJobsPanelItem } from '../../lib/backgroundJobsPanel'
import { compactToolCallStatusLine } from '../../lib/toolCallDisplay'

const props = defineProps<{
  conversationId: string
}>()

const chat = useChatStore()

const expanded = ref(false)
const armedCancel = ref(false)
let disarmTimer: ReturnType<typeof setTimeout> | null = null

const conv = computed(
  () =>
    (chat.current?.id === props.conversationId ? chat.current : undefined)
    ?? chat.conversations.find(c => c.id === props.conversationId)
)

/** 生成回复时主界面已有停止按钮与过程展示，面板让位，避免两套操作打架。 */
const generating = computed(() => chat.isConversationGenerating(props.conversationId))
const count = computed(() => chat.backgroundJobCount(props.conversationId))
const visible = computed(() => count.value > 0 && !generating.value)

const rows = computed(() => collectLiveBackgroundJobs(conv.value?.messages))

/** 可一键取消的 host 行（拿到了 jobId）。 */
const cancelableHosts = computed(
  () => rows.value.filter(r => r.kind === 'host' && !!r.jobId) as Extract<BackgroundJobsPanelItem, { kind: 'host' }>[]
)

function rowLabel(item: BackgroundJobsPanelItem): string {
  return compactToolCallStatusLine(item.toolCall, chat.current?.workspaceRoot) || '后台任务'
}

function cancelJob(item: Extract<BackgroundJobsPanelItem, { kind: 'host' }>) {
  if (!item.jobId) return
  void chat.cancelBackgroundJob(item.jobId)
}

function endWait() {
  void chat.endWaitKeepBackground()
}

function disarmCancel() {
  if (disarmTimer) {
    clearTimeout(disarmTimer)
    disarmTimer = null
  }
  armedCancel.value = false
}

/** 全部取消采用两步确认，避免误触后一次性杀掉所有后台任务。 */
function onCancelAll() {
  if (cancelableHosts.value.length === 0) return
  if (!armedCancel.value) {
    armedCancel.value = true
    disarmTimer = setTimeout(disarmCancel, 3000)
    return
  }
  disarmCancel()
  for (const item of cancelableHosts.value) {
    if (item.jobId) void chat.cancelBackgroundJob(item.jobId)
  }
}

onBeforeUnmount(disarmCancel)
</script>

<template>
  <div
    v-if="visible"
    class="composer-companion mb-2 overflow-hidden rounded-2xl border border-border bg-[hsl(var(--composer-bg))]"
    role="status"
  >
    <div class="flex items-center gap-1 pl-3 pr-2 py-1">
      <button
        type="button"
        class="flex min-w-0 flex-1 items-center gap-2 py-1 text-left cursor-pointer"
        :aria-expanded="expanded"
        @click="expanded = !expanded"
      >
        <span class="inline-block h-1.5 w-1.5 shrink-0 rounded-full bg-accent animate-pulse" aria-hidden="true" />
        <span class="text-[12px] font-medium text-muted flex-1 min-w-0 truncate">后台任务</span>
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-hover text-muted shrink-0 tabular-nums">
          {{ count }}
        </span>
        <span class="hidden sm:inline text-[10px] text-muted/80 shrink-0">执行中，不影响继续对话</span>
        <ChevronDown
          class="h-3.5 w-3.5 text-muted shrink-0 transition-transform duration-200"
          :class="expanded ? 'rotate-180' : ''"
          aria-hidden="true"
        />
      </button>
      <button
        type="button"
        class="shrink-0 rounded-md px-2 py-1 text-[11px] font-medium border-0 cursor-pointer transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        :class="armedCancel ? 'bg-danger text-white hover:opacity-90' : 'text-danger/80 hover:bg-danger/10'"
        :disabled="cancelableHosts.length === 0"
        :title="armedCancel ? '再次点击确认取消全部后台任务' : '取消全部后台任务'"
        aria-label="取消全部后台任务"
        @click.stop="onCancelAll"
      >
        {{ armedCancel ? '确认取消' : '全部取消' }}
      </button>
    </div>

    <ul
      v-show="expanded"
      class="border-t border-border divide-y divide-border max-h-40 overflow-y-auto"
    >
      <li
        v-for="item in rows"
        :key="item.key"
        class="flex items-center gap-2 px-3 py-1.5 text-[12px]"
      >
        <span class="min-w-0 flex-1 truncate text-foreground">{{ rowLabel(item) }}</span>
        <button
          v-if="item.kind === 'host' && item.jobId"
          type="button"
          class="shrink-0 border-0 bg-transparent px-1 py-0.5 text-[11px] text-danger/80 hover:text-danger cursor-pointer transition-colors"
          :title="'只结束这条后台任务'"
          @click.stop="cancelJob(item)"
        >结束任务</button>
        <button
          v-else-if="item.kind === 'await'"
          type="button"
          class="shrink-0 border-0 bg-transparent px-1 py-0.5 text-[11px] text-danger/80 hover:text-danger cursor-pointer transition-colors"
          :title="'结束等待，后台任务继续跑'"
          @click.stop="endWait"
        >结束等待</button>
      </li>
      <li v-if="!rows.length" class="px-3 py-2 text-[11px] text-muted">
        任务仍在运行，详情见上方消息记录
      </li>
    </ul>
  </div>
</template>
