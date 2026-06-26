import type { TaskBoardItem } from '../types/chat'

/** Milestone row label — UI shows `title` as planned at init. */
export function milestoneTitle(item: TaskBoardItem): string {
  return item.title?.trim() || item.id
}
