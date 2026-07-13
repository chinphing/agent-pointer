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

function planIndex(globals: TaskBoardItem[]): number {
  return globals.findIndex(r => r.id === 'g_plan')
}

function deliverIndex(globals: TaskBoardItem[]): number {
  return globals.findIndex(r => r.id === 'g_deliver')
}

/** Loop item rows (`wi_*`) between g_plan and g_deliver. */
export function taskBoardLoopItemRows(
  doc: TaskBoardDocument | null | undefined
): TaskBoardItem[] {
  const globals = taskBoardGlobalMilestones(doc)
  const deliverIdx = deliverIndex(globals)
  if (deliverIdx < 0) return []
  const p = planIndex(globals)
  if (p < 0 || p + 1 >= deliverIdx) return []
  return globals
    .slice(p + 1, deliverIdx)
    .filter(r => r.id.startsWith('wi_'))
}

/** Loop board: g_plan + wi_* + g_deliver in global_milestones. */
export function taskBoardIsLoopMilestoneBoard(
  doc: TaskBoardDocument | null | undefined
): boolean {
  if (!doc) return false
  return taskBoardLoopItemRows(doc).some(r => r.id.startsWith('wi_'))
}

function loopExecMet(doc: TaskBoardDocument | null | undefined): boolean {
  const rows = taskBoardLoopItemRows(doc)
  if (!rows.length) return false
  return rows.every(
    r => r.status === 'done' || r.status === 'failed' || r.status === 'cancelled'
  )
}

/** Mirrors host inject projection: linear step globals, loop exec, or deliver-only. */
export type TaskBoardMilestoneViewMode = 'step' | 'queue_exec' | 'queue_deliver'

export function taskBoardMilestoneViewMode(
  doc: TaskBoardDocument | null | undefined
): TaskBoardMilestoneViewMode {
  if (taskBoardIsLoopMilestoneBoard(doc)) {
    return loopExecMet(doc) ? 'queue_deliver' : 'queue_exec'
  }
  return 'step'
}

/** Milestone rows shown in UI — full `global_milestones` ladder (no phase filtering). */
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

/** Loop progress from wi_* rows (done+failed / total). */
export function taskBoardLoopItemsProgress(
  doc: TaskBoardDocument | null | undefined
): string | null {
  const rows = taskBoardLoopItemRows(doc)
  if (!rows.length) return null
  const terminal = rows.filter(
    r => r.status === 'done' || r.status === 'failed'
  ).length
  return `${terminal}/${rows.length}`
}

/** @deprecated Use taskBoardLoopItemsProgress */
export const taskBoardWorkItemsProgress = taskBoardLoopItemsProgress

/** Stable key for UI refresh after board patches. */
export function taskBoardDocumentSyncKey(
  doc: TaskBoardDocument | null | undefined
): string {
  if (!doc) return ''
  const wi = taskBoardLoopItemRows(doc)
    .map(r => `${r.id}:${r.status}`)
    .join('|')
  const globals = taskBoardGlobalMilestones(doc)
    .map(i => `${i.id}:${i.status}`)
    .join('|')
  return `${doc.meta?.status ?? ''}:${wi}:${globals}`
}

/** Done / total global milestone count, e.g. `1/3`. */
export function taskBoardMilestoneProgress(
  document: TaskBoardDocument | null | undefined
): string {
  return taskBoardVisibleMilestoneProgress(document)
}

/** True when the agent has initialized a task board (goal or milestones). */
export function hasTaskBoardContent(doc: TaskBoardDocument | null | undefined): boolean {
  if (!doc) return false
  if (taskBoardGlobalMilestones(doc).length > 0) return true
  return !!(doc.meta?.goal?.trim())
}
