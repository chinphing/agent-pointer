import type { TaskBoardDocument, TaskBoardItem } from '../types/chat'

/** v4 `global_milestones` with legacy v3 `board` fallback. */
export function taskBoardGlobalMilestones(
  doc: TaskBoardDocument | null | undefined
): TaskBoardItem[] {
  if (!doc) return []
  const v4 = doc.global_milestones
  if (Array.isArray(v4) && v4.length > 0) return v4
  return doc.board ?? []
}

/** Milestone rows shown in UI — full `global_milestones` ladder. */
export function taskBoardVisibleMilestones(
  doc: TaskBoardDocument | null | undefined
): TaskBoardItem[] {
  return taskBoardGlobalMilestones(doc)
}

/** Done+failed / total for the visible milestone ladder. */
export function taskBoardVisibleMilestoneProgress(
  doc: TaskBoardDocument | null | undefined
): string {
  const rows = taskBoardVisibleMilestones(doc)
  if (!rows.length) return '0/0'
  const terminal = rows.filter(
    i => i.status === 'done' || i.status === 'failed'
  ).length
  return `${terminal}/${rows.length}`
}

/** True when the agent has initialized a task board (goal or milestones). */
export function hasTaskBoardContent(doc: TaskBoardDocument | null | undefined): boolean {
  if (!doc) return false
  if (taskBoardGlobalMilestones(doc).length > 0) return true
  return !!(doc.meta?.goal?.trim())
}
