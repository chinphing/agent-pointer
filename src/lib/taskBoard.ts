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
  const items = taskBoardGlobalMilestones(document)
  if (!items.length) return '0/0'
  const done = items.filter(i => i.status === 'done').length
  return `${done}/${items.length}`
}

/** True when the agent has initialized a task board (goal or milestones). */
export function hasTaskBoardContent(doc: TaskBoardDocument | null | undefined): boolean {
  if (!doc) return false
  if (taskBoardGlobalMilestones(doc).length > 0) return true
  if (taskBoardItemMilestones(doc).length > 0) return true
  return !!(doc.meta?.goal?.trim())
}
