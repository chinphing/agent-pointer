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

/** v4 per-item SOP template rows. */
export function taskBoardItemMilestones(
  doc: TaskBoardDocument | null | undefined
): TaskBoardItem[] {
  if (!doc) return []
  return doc.item_milestones ?? []
}

/** Mirrors host inject projection: step globals, queue item SOP, or deliver-only. */
export type TaskBoardMilestoneViewMode = 'step' | 'queue_exec' | 'queue_deliver'

export function taskBoardMilestoneViewMode(
  doc: TaskBoardDocument | null | undefined
): TaskBoardMilestoneViewMode {
  if (!doc || !taskBoardHasWorkItems(doc)) return 'step'
  const gExec = taskBoardGlobalMilestones(doc).find(r => r.id === 'g_exec')
  if (gExec?.status === 'done') return 'queue_deliver'
  return 'queue_exec'
}

/** Milestone rows shown in UI — same ladder as model inject, not raw globals + SOP. */
export function taskBoardVisibleMilestones(
  doc: TaskBoardDocument | null | undefined
): TaskBoardItem[] {
  if (!doc) return []
  const mode = taskBoardMilestoneViewMode(doc)
  if (mode === 'queue_exec') {
    return taskBoardItemMilestones(doc).filter(r => !r.id.startsWith('deliver_'))
  }
  if (mode === 'queue_deliver') {
    const deliver = taskBoardGlobalMilestones(doc).find(r => r.id === 'g_deliver')
    return deliver ? [deliver] : []
  }
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

/** Stable key for refetching work_items stats after board patches. */
export function taskBoardDocumentSyncKey(
  doc: TaskBoardDocument | null | undefined
): string {
  if (!doc) return ''
  const globals = taskBoardGlobalMilestones(doc)
    .map(i => `${i.id}:${i.status}`)
    .join('|')
  const items = taskBoardItemMilestones(doc)
    .map(i => `${i.id}:${i.status}`)
    .join('|')
  return `${doc.meta?.status ?? ''}:${globals}::${items}`
}

/** True when the board uses external work_items (v4 meta or legacy row flags). */
export function taskBoardHasWorkItems(doc: TaskBoardDocument | null | undefined): boolean {
  if (!doc) return false
  const mode = doc.meta?.work_item_mode
  if (mode === 'enumerated' || mode === 'dynamic') return true
  return taskBoardGlobalMilestones(doc).some(row => hasWorkItemsBatch(row))
}

function hasWorkItemsBatch(row: TaskBoardItem): boolean {
  return (
    row.work_item_mode === 'enumerated'
    || row.work_item_mode === 'dynamic'
    || row.dynamic_quota != null
  )
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
  if (taskBoardItemMilestones(doc).length > 0) return true
  return !!(doc.meta?.goal?.trim())
}
