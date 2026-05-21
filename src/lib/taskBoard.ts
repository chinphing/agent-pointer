import type { TaskBoardDocument } from '../types/chat'

/** True when the agent has initialized a task board (goal or milestones). */
export function hasTaskBoardContent(doc: TaskBoardDocument | null | undefined): boolean {
  if (!doc) return false
  if ((doc.board?.length ?? 0) > 0) return true
  return !!(doc.meta?.goal?.trim())
}
