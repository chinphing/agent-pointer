<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { LayoutList, CheckCircle2, Circle, Loader2, XCircle, Ban } from 'lucide-vue-next'
import type { TaskBoardDocument, TaskBoardItem } from '../../types/chat'
import {
  hasTaskBoardContent,
  taskBoardDocumentSyncKey,
  taskBoardGlobalMilestones,
  taskBoardHasWorkItems,
  taskBoardMilestoneViewMode,
  taskBoardVisibleMilestones,
  taskBoardVisibleMilestoneProgress,
  taskBoardWorkItemsProgress
} from '../../lib/taskBoard'
import { milestoneTitle } from '../../lib/taskBoardDisplay'
import { useWorkItemsList } from '../../composables/useWorkItemsList'
import WorkItemsBatchList from './WorkItemsBatchList.vue'

const props = defineProps<{
  document: TaskBoardDocument | null
  isActive?: boolean
  childBoards?: Record<string, TaskBoardDocument>
  conversationId?: string | null
  taskId?: string
  workItemsEnabled?: boolean
}>()

const goal = computed(() => props.document?.meta?.goal?.trim() ?? '')
const metaStatus = computed(() => props.document?.meta?.status ?? 'running')
const viewMode = computed(() => taskBoardMilestoneViewMode(props.document))
const visibleMilestones = computed(() => taskBoardVisibleMilestones(props.document))
const documentSyncKey = computed(() => taskBoardDocumentSyncKey(props.document))

const wiEnabled = computed(() => props.workItemsEnabled === true && !!props.conversationId?.trim())
const showWorkItemStats = computed(
  () => wiEnabled.value && viewMode.value === 'queue_exec' && taskBoardHasWorkItems(props.document)
)

const wiStoreList = useWorkItemsList({
  conversationId: toRef(props, 'conversationId'),
  taskId: toRef(props, 'taskId'),
  batchId: ref(''),
  enabled: showWorkItemStats,
  refreshKey: documentSyncKey
})

watch(
  () => [showWorkItemStats.value, documentSyncKey.value] as const,
  ([enabled]) => {
    if (enabled) void wiStoreList.loadStats()
  },
  { immediate: true }
)

const milestoneProgress = computed(() => {
  const fromDoc = taskBoardWorkItemsProgress(props.document)
  if (showWorkItemStats.value && fromDoc) return fromDoc
  if (showWorkItemStats.value && wiStoreList.stats.value) {
    const s = wiStoreList.stats.value
    return `${s.done + s.failed}/${s.total}`
  }
  return taskBoardVisibleMilestoneProgress(props.document)
})

const wiBatches = computed(() =>
  taskBoardGlobalMilestones(props.document).filter(row => hasWorkItemsBatch(row))
)

const showWorkItemsPanel = computed(
  () => wiEnabled.value && (taskBoardHasWorkItems(props.document) || wiBatches.value.length > 0)
)

const openBatchId = ref<string | null>(null)

watch(
  () => props.document?.meta?.work_items_seeded_rows,
  seeded => {
    if (typeof seeded === 'number' && seeded > 0 && openBatchId.value == null) {
      openBatchId.value = '__store__'
    }
  },
  { immediate: true }
)

const childBoardsWithContent = computed(() => {
  if (!props.isActive) return {}
  const src = props.childBoards ?? {}
  return Object.fromEntries(
    Object.entries(src).filter(([, doc]) => hasTaskBoardContent(doc))
  )
})

function hasWorkItemsBatch(row: TaskBoardItem): boolean {
  return (
    row.work_item_mode === 'enumerated'
    || row.work_item_mode === 'dynamic'
    || row.dynamic_quota != null
  )
}

function batchRefreshKey(row: TaskBoardItem): string {
  return `${documentSyncKey.value}:${row.id}:${row.status}:${row.progress ?? ''}`
}

function batchTitle(row: TaskBoardItem): string {
  const t = row.title?.trim()
  return t || row.id
}

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

function rowLabel(item: TaskBoardItem): string {
  return milestoneTitle(item)
}

function onBatchOpen(batchId: string, open: boolean) {
  openBatchId.value = open ? batchId : null
}

</script>

<template>
  <details
    v-if="document && hasTaskBoardContent(document)"
    class="task-board-curtain rounded-b-2xl rounded-t-lg border border-border bg-card overflow-hidden w-fit max-w-[80%] min-w-[240px] shadow-sm"
  >
    <summary
      class="cursor-pointer select-none px-3 py-2 flex items-center gap-2 list-none hover:bg-hover transition"
    >
      <LayoutList class="w-4 h-4 text-accent shrink-0" />
      <span class="text-[13px] font-medium text-foreground truncate flex-1">
        {{ goal || '任务板' }}
      </span>
      <span class="text-[11px] text-muted shrink-0 tabular-nums">{{ milestoneProgress }}</span>
      <span
        v-if="isActive"
        class="text-[10px] px-1.5 py-0.5 rounded bg-success/10 text-success shrink-0"
      >
        active
      </span>
      <span class="text-[10px] px-1.5 py-0.5 rounded bg-accent-muted text-accent shrink-0">{{ metaStatus }}</span>
    </summary>
    <div class="border-t border-border px-3 py-2 space-y-1 max-h-48 overflow-y-auto">
      <div
        v-for="item in visibleMilestones"
        :key="item.id"
        class="flex items-start gap-2 text-[12px] py-1 min-h-[1.5rem]"
      >
        <component :is="statusIcon(item.status)" class="w-3.5 h-3.5 shrink-0 mt-0.5" :class="statusClass(item.status)" />
        <div class="min-w-0 flex-1 text-foreground leading-snug break-words">{{ rowLabel(item) }}</div>
      </div>
      <div v-if="!visibleMilestones.length" class="text-[11px] text-muted py-2">暂无任务步骤</div>
    </div>
    <div
      v-if="showWorkItemsPanel && conversationId"
      class="border-t border-border px-3 py-2 space-y-1 max-h-56 overflow-y-auto bg-accent-muted/10"
    >
      <WorkItemsBatchList
        v-if="taskBoardHasWorkItems(document)"
        :key="`wi-store-${taskId ?? 'main'}`"
        :conversation-id="conversationId"
        :task-id="taskId"
        batch-id=""
        :batch-title="goal || '工作项'"
        :enabled="wiEnabled"
        :refresh-key="documentSyncKey"
        :open="openBatchId === '__store__'"
        @update:open="(v) => onBatchOpen('__store__', v)"
      />
      <WorkItemsBatchList
        v-for="batch in wiBatches"
        :key="batch.id"
        :conversation-id="conversationId"
        :task-id="taskId"
        :batch-id="batch.id"
        :batch-title="batchTitle(batch)"
        :enabled="wiEnabled"
        :refresh-key="batchRefreshKey(batch)"
        :open="openBatchId === batch.id"
        @update:open="(v) => onBatchOpen(batch.id, v)"
      />
    </div>
    <div
      v-for="(child, taskIdKey) in childBoardsWithContent"
      :key="taskIdKey"
      class="border-t border-border px-3 py-2 bg-accent-muted/20"
    >
      <div class="text-[11px] text-muted mb-1">子任务 {{ taskIdKey }}</div>
      <div
        v-for="row in taskBoardVisibleMilestones(child)"
        :key="row.id"
        class="flex items-start gap-2 text-[11px] py-0.5 min-h-[1.25rem]"
      >
        <component :is="statusIcon(row.status)" class="w-3 h-3 shrink-0 mt-0.5" :class="statusClass(row.status)" />
        <div class="min-w-0 flex-1 leading-snug break-words text-foreground">{{ rowLabel(row) }}</div>
      </div>
    </div>
  </details>
</template>

<style scoped>
details > summary::-webkit-details-marker { display: none; }
</style>
