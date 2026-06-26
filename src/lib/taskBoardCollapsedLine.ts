import type { TaskBoardDocument } from '../types/chat'
import {
  taskBoardHasWorkItems,
  taskBoardVisibleMilestones,
  taskBoardVisibleMilestoneProgress,
  taskBoardWorkItemsProgress
} from './taskBoard'
import { milestoneTitle } from './taskBoardDisplay'

export interface TaskBoardCompactSummary {
  goal: string
  /** Primary line: in-progress milestone summary, else goal. */
  taskLine: string
  /** Done / total milestones, e.g. `1/3`. */
  progress: string
  doneCount: number
  total: number
  currentStep: string | null
  /** One-line summary for tooltip / accessibility. */
  fullLine: string
}

/** Structured summary for compact dock bar. */
export function taskBoardCompactSummary(
  document: TaskBoardDocument | null | undefined
): TaskBoardCompactSummary | null {
  const items = taskBoardVisibleMilestones(document)
  if (!items.length && !document?.meta?.goal?.trim()) return null

  const goal = document?.meta?.goal?.trim() || '任务板'
  const wiProgress =
    taskBoardHasWorkItems(document) ? taskBoardWorkItemsProgress(document) : null
  const progress = wiProgress ?? taskBoardVisibleMilestoneProgress(document)
  let doneCount: number
  let total: number
  if (wiProgress && document?.meta) {
    const done = document.meta.work_items_done ?? 0
    const failed = document.meta.work_items_failed ?? 0
    doneCount = done + failed
    total = document.meta.work_items_total ?? 0
  } else {
    doneCount = items.filter(
      i => i.status === 'done' || i.status === 'failed'
    ).length
    total = items.length
  }
  const inProgress = items.find(i => i.status === 'in_progress')
  const currentStep = inProgress ? milestoneTitle(inProgress) : null
  const taskLine = currentStep ?? goal

  return { goal, taskLine, progress, doneCount, total, currentStep, fullLine: taskLine }
}

/** One-line collapsed summary for task board. */
export function taskBoardCollapsedLine(document: TaskBoardDocument | null | undefined): string | null {
  return taskBoardCompactSummary(document)?.fullLine ?? null
}
