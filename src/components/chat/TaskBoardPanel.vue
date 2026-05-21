<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LayoutList, CheckCircle2, Circle, Loader2, XCircle, Ban } from 'lucide-vue-next'
import type { TaskBoardDocument, TaskBoardItem } from '../../types/chat'
import { hasTaskBoardContent } from '../../lib/taskBoard'

const props = defineProps<{
  document: TaskBoardDocument | null
  childBoards?: Record<string, TaskBoardDocument>
}>()

const open = ref(true)

const goal = computed(() => props.document?.meta?.goal?.trim() ?? '')
const metaStatus = computed(() => props.document?.meta?.status ?? 'running')
const items = computed(() => props.document?.board ?? [])

const childBoardsWithContent = computed(() => {
  const src = props.childBoards ?? {}
  return Object.fromEntries(
    Object.entries(src).filter(([, doc]) => hasTaskBoardContent(doc))
  )
})

const doneCount = computed(() =>
  items.value.filter(i => i.status === 'done').length
)

function statusIcon(status: string) {
  switch (status) {
    case 'done': return CheckCircle2
    case 'in_progress': return Loader2
    case 'failed': return XCircle
    case 'cancelled': return Ban
    default: return Circle
  }
}

function statusClass(status: string): string {
  switch (status) {
    case 'done': return 'text-success'
    case 'in_progress': return 'text-accent animate-spin'
    case 'failed': return 'text-danger'
    case 'cancelled': return 'text-muted'
    default: return 'text-muted'
  }
}

function statusLabel(status: string): string {
  const m: Record<string, string> = {
    pending: '待开始',
    ready: '就绪',
    in_progress: '进行中',
    done: '完成',
    failed: '失败',
    cancelled: '已取消'
  }
  return m[status] ?? status
}

watch(
  () => props.document,
  doc => {
    if (doc && (doc.board?.length ?? 0) > 0) open.value = true
  }
)
</script>

<template>
  <details
    v-if="document && hasTaskBoardContent(document)"
    class="rounded-lg border border-border bg-card mb-3 overflow-hidden"
    :open="open"
  >
    <summary
      class="cursor-pointer select-none px-3 py-2 flex items-center gap-2 list-none hover:bg-hover transition"
    >
      <LayoutList class="w-4 h-4 text-accent shrink-0" />
      <span class="text-[13px] font-medium text-foreground truncate flex-1">
        {{ goal || '任务板' }}
      </span>
      <span class="text-[11px] text-muted shrink-0">{{ doneCount }}/{{ items.length }}</span>
      <span class="text-[10px] px-1.5 py-0.5 rounded bg-accent-muted text-accent shrink-0">{{ metaStatus }}</span>
    </summary>
    <div class="border-t border-border px-3 py-2 space-y-1.5 max-h-48 overflow-y-auto">
      <div
        v-for="item in items"
        :key="item.id"
        class="flex items-start gap-2 text-[12px] py-1"
      >
        <component :is="statusIcon(item.status)" class="w-3.5 h-3.5 mt-0.5 shrink-0" :class="statusClass(item.status)" />
        <div class="min-w-0 flex-1">
          <div class="text-foreground truncate">{{ item.title }}</div>
          <div class="text-[10px] text-muted">{{ statusLabel(item.status) }}</div>
          <div v-if="item.depends_on?.length" class="text-[10px] text-muted">依赖: {{ item.depends_on.join(', ') }}</div>
        </div>
      </div>
      <div v-if="!items.length" class="text-[11px] text-muted py-2">暂无里程碑</div>
    </div>
    <div
      v-for="(child, taskId) in childBoardsWithContent"
      :key="taskId"
      class="border-t border-border px-3 py-2 bg-accent-muted/20"
    >
      <div class="text-[11px] text-muted mb-1">子任务 {{ taskId }}</div>
      <div
        v-for="row in child.board"
        :key="row.id"
        class="flex items-center gap-2 text-[11px] py-0.5"
      >
        <component :is="statusIcon(row.status)" class="w-3 h-3 shrink-0" :class="statusClass(row.status)" />
        <span class="truncate text-foreground">{{ row.title }}</span>
      </div>
    </div>
  </details>
</template>

<style scoped>
details > summary::-webkit-details-marker { display: none; }
</style>
