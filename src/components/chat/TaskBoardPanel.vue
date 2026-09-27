<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ListChecks, CheckCircle2, Circle, Loader2, XCircle, Ban, ChevronDown, ChevronRight } from 'lucide-vue-next'
import type { TaskBoardDocument, TaskBoardItem } from '../../types/chat'
import {
  hasTaskBoardContent,
  milestoneShowsRunning,
  nestedBoardMetaStatus,
  taskBoardCurrentMilestone,
  taskBoardExecutionLabel,
  taskBoardVisibleMilestones,
  taskBoardVisibleMilestoneProgress
} from '../../lib/taskBoard'
import { milestoneRowLabel } from '../../lib/taskBoardDisplay'


const { t } = useI18n()
const props = defineProps<{
  document: TaskBoardDocument | null
  isActive?: boolean
  childBoards?: Record<string, TaskBoardDocument>
  conversationId?: string | null
  taskId?: string
  /** Nested board: follow the sub-agent trace so leftover meta.running is not painted. */
  agentStatus?: string
}>()

const expanded = ref(false)

const goal = computed(() => props.document?.meta?.goal?.trim() ?? '')
const metaStatus = computed(() =>
  nestedBoardMetaStatus(props.document?.meta?.status, props.agentStatus)
)
const visibleMilestones = computed(() => taskBoardVisibleMilestones(props.document))
const currentMilestone = computed(() => {
  const meta = metaStatus.value.trim().toLowerCase()
  if (
    meta === 'completed'
    || meta === 'failed'
    || meta === 'cancelled'
    || meta === 'canceled'
  ) {
    return null
  }
  return taskBoardCurrentMilestone(props.document)
})
const milestoneProgress = computed(() => taskBoardVisibleMilestoneProgress(props.document))
const executionLabel = computed(() => taskBoardExecutionLabel(metaStatus.value))
const boardFailed = computed(() => executionLabel.value === t('taskBoard.failed'))

const summaryTitle = computed(() => goal.value || t('taskBoard.boardTitle'))
const summaryAria = computed(() => {
  const parts = [summaryTitle.value, t('taskBoard.progressAria', { progress: milestoneProgress.value })]
  if (executionLabel.value) parts.push(executionLabel.value)
  return parts.join('，')
})

const childBoardsWithContent = computed(() => {
  if (!props.isActive) return {}
  const src = props.childBoards ?? {}
  return Object.fromEntries(
    Object.entries(src).filter(([, doc]) => hasTaskBoardContent(doc))
  )
})

function statusIcon(
  item: TaskBoardItem,
  current: TaskBoardItem | null = currentMilestone.value,
  boardMeta: string = metaStatus.value
) {
  if (milestoneShowsRunning(item, current, boardMeta)) return Loader2
  switch (item.status) {
    case 'done': return CheckCircle2
    case 'failed': return XCircle
    case 'cancelled': return Ban
    default: return Circle
  }
}

function statusClass(
  item: TaskBoardItem,
  current: TaskBoardItem | null = currentMilestone.value,
  boardMeta: string = metaStatus.value
): string {
  if (milestoneShowsRunning(item, current, boardMeta)) return 'text-muted/70 animate-spin'
  switch (item.status) {
    case 'done': return 'text-success'
    case 'failed': return 'text-danger'
    case 'cancelled': return 'text-muted'
    default: return 'text-muted/70'
  }
}

function rowLabel(item: TaskBoardItem): string {
  return milestoneRowLabel(item)
}

function childGoal(doc: TaskBoardDocument): string {
  return doc.meta?.goal?.trim() || t('taskBoard.subtask')
}

function toggleExpanded() {
  expanded.value = !expanded.value
}
</script>

<template>
  <div
    v-if="document && hasTaskBoardContent(document)"
    class="task-board-panel min-w-0 w-fit max-w-full"
  >
    <button
      type="button"
      class="tool-call-trigger group flex flex-col items-start w-fit max-w-full text-left py-0.5 transition cursor-pointer"
      :aria-expanded="expanded"
      :aria-label="summaryAria"
      @click="toggleExpanded"
    >
      <span class="collapsed-run-hover-pill collapsed-run-summary-pill">
        <ListChecks class="h-3.5 w-3.5 shrink-0 text-muted/70" aria-hidden="true" />
        <span class="min-w-0 text-[13px] text-muted group-hover:text-foreground truncate">
          {{ summaryTitle }}
        </span>
        <span class="shrink-0 text-[10px] text-muted/45 tabular-nums">{{ milestoneProgress }}</span>
        <span
          v-if="executionLabel"
          class="shrink-0 text-[13px]"
          :class="boardFailed ? 'text-danger' : 'text-muted/45'"
        >{{ executionLabel }}</span>
        <component
          :is="expanded ? ChevronDown : ChevronRight"
          class="tool-call-chevron h-3 w-3 shrink-0 text-muted"
          aria-hidden="true"
        />
      </span>
    </button>

    <div
      v-if="expanded"
      class="task-board-steps pl-5 space-y-0.5 pt-0.5"
    >
      <div
        v-for="item in visibleMilestones"
        :key="item.id"
        class="flex items-start gap-1.5 text-[13px] py-0.5 min-w-0"
      >
        <component
          :is="statusIcon(item)"
          class="w-3.5 h-3.5 shrink-0 mt-0.5"
          :class="statusClass(item)"
        />
        <div class="min-w-0 flex-1 text-muted leading-snug break-words">
          {{ rowLabel(item) }}
        </div>
      </div>
      <div
        v-if="!visibleMilestones.length"
        class="text-[13px] text-muted py-0.5"
      >{{ t('taskBoard.noSteps') }}</div>
      <div
        v-for="(child, taskIdKey) in childBoardsWithContent"
        :key="taskIdKey"
        class="pt-1 space-y-0.5"
      >
        <div class="text-[13px] text-muted truncate">{{ childGoal(child) }}</div>
        <div
          v-for="row in taskBoardVisibleMilestones(child)"
          :key="row.id"
          class="flex items-start gap-1.5 text-[13px] py-0.5 min-w-0"
        >
          <component
            :is="statusIcon(row, taskBoardCurrentMilestone(child), child.meta?.status)"
            class="w-3.5 h-3.5 shrink-0 mt-0.5"
            :class="statusClass(row, taskBoardCurrentMilestone(child), child.meta?.status)"
          />
          <div class="min-w-0 flex-1 leading-snug break-words text-muted">
            {{ milestoneRowLabel(row) }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
