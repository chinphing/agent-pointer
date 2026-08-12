<script setup lang="ts">
import { computed } from 'vue'
import { LayoutList, CheckCircle2, Circle, Loader2, XCircle, Ban } from 'lucide-vue-next'
import type { TaskBoardDocument, TaskBoardItem } from '../../types/chat'
import {
  hasTaskBoardContent,
  milestoneShowsRunning,
  taskBoardCurrentMilestone,
  taskBoardVisibleMilestones,
  taskBoardVisibleMilestoneProgress
} from '../../lib/taskBoard'
import { milestoneRowLabel } from '../../lib/taskBoardDisplay'

const props = defineProps<{
  document: TaskBoardDocument | null
  isActive?: boolean
  childBoards?: Record<string, TaskBoardDocument>
  conversationId?: string | null
  taskId?: string
}>()

const goal = computed(() => props.document?.meta?.goal?.trim() ?? '')
const metaStatus = computed(() => props.document?.meta?.status ?? 'running')
const visibleMilestones = computed(() => taskBoardVisibleMilestones(props.document))
const currentMilestone = computed(() => taskBoardCurrentMilestone(props.document))

const milestoneProgress = computed(() => taskBoardVisibleMilestoneProgress(props.document))

/** Current work row (in_progress, or ready fallback) — closed-summary spinner. */
const hasRunning = computed(() => currentMilestone.value != null)

const childBoardsWithContent = computed(() => {
  if (!props.isActive) return {}
  const src = props.childBoards ?? {}
  return Object.fromEntries(
    Object.entries(src).filter(([, doc]) => hasTaskBoardContent(doc))
  )
})

function statusIcon(item: TaskBoardItem, current: TaskBoardItem | null = currentMilestone.value) {
  if (milestoneShowsRunning(item, current)) return Loader2
  switch (item.status) {
    case 'done': return CheckCircle2
    case 'failed': return XCircle
    case 'cancelled': return Ban
    default: return Circle
  }
}

function statusClass(item: TaskBoardItem, current: TaskBoardItem | null = currentMilestone.value): string {
  if (milestoneShowsRunning(item, current)) return 'text-accent animate-spin'
  switch (item.status) {
    case 'done': return 'text-success'
    case 'failed': return 'text-danger'
    case 'cancelled': return 'text-muted'
    default: return 'text-muted'
  }
}

function rowLabel(item: TaskBoardItem): string {
  return milestoneRowLabel(item)
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
      <Loader2
        v-if="hasRunning"
        class="w-4 h-4 text-accent shrink-0 animate-spin"
        aria-hidden="true"
      />
      <LayoutList v-else class="w-4 h-4 text-accent shrink-0" />
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
        <component :is="statusIcon(item)" class="w-3.5 h-3.5 shrink-0 mt-0.5" :class="statusClass(item)" />
        <div class="min-w-0 flex-1 text-foreground leading-snug break-words">
          <div>{{ rowLabel(item) }}</div>
        </div>
      </div>
      <div v-if="!visibleMilestones.length" class="text-[11px] text-muted py-2">暂无任务步骤</div>
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
        <component
          :is="statusIcon(row, taskBoardCurrentMilestone(child))"
          class="w-3 h-3 shrink-0 mt-0.5"
          :class="statusClass(row, taskBoardCurrentMilestone(child))"
        />
        <div class="min-w-0 flex-1 leading-snug break-words text-foreground">
          <div>{{ milestoneRowLabel(row) }}</div>
        </div>
      </div>
    </div>
  </details>
</template>

<style scoped>
details > summary::-webkit-details-marker {
  display: none;
}
</style>
