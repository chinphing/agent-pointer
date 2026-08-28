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

/**
 * Current work row for UI — mirrors inject: prefer `in_progress`, else first
 * `ready`/`pending` while meta is still running (host may not have promoted yet).
 */
export function taskBoardCurrentMilestone(
  doc: TaskBoardDocument | null | undefined
): TaskBoardItem | null {
  const rows = taskBoardVisibleMilestones(doc)
  const meta = (doc?.meta?.status ?? 'running').trim().toLowerCase()
  if (meta === 'completed' || meta === 'failed') return null
  const inProgress = rows.find(i => i.status === 'in_progress')
  if (inProgress) return inProgress
  return (
    rows.find(i => i.status === 'ready')
    ?? rows.find(i => i.status === 'pending')
    ?? null
  )
}

/** True when this row should show the running spinner. */
export function milestoneShowsRunning(
  item: TaskBoardItem,
  current: TaskBoardItem | null,
  metaStatus?: string
): boolean {
  const meta = (metaStatus ?? '').trim()
  if (meta === 'completed' || meta === 'failed') return false
  if (item.status === 'in_progress') return true
  if (!current || current.id !== item.id) return false
  return item.status === 'ready' || item.status === 'pending'
}

/** True when the agent has initialized a task board (goal or milestones). */
export function hasTaskBoardContent(doc: TaskBoardDocument | null | undefined): boolean {
  if (!doc) return false
  if (taskBoardGlobalMilestones(doc).length > 0) return true
  return !!(doc.meta?.goal?.trim())
}

/**
 * User-facing board execution status. Never "active" (that flag is host
 * bookkeeping for the current parent board, not something to paint).
 * Completed boards omit a label — progress `n/m` is enough.
 */
export function taskBoardExecutionLabel(status: string | undefined): string | null {
  const s = (status ?? '').trim().toLowerCase()
  if (s === 'failed') return '失败'
  if (s === 'paused') return '已暂停'
  if (s === 'completed') return null
  if (s === 'cancelled' || s === 'canceled') return '已取消'
  if (s === 'running' || s === 'active' || s === '') return '执行中'
  return null
}
