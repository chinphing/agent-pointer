import type { TaskBoardItem } from '../types/chat'

/** Milestone row label — UI shows `title` as planned at init. */
export function milestoneTitle(item: TaskBoardItem): string {
  return item.title?.trim() || item.id
}

const TERMINAL_STATUSES = new Set(['done', 'failed', 'cancelled'])

/** Outcome note for terminal rows (matches inject list `remark` projection). */
export function milestoneRemark(item: TaskBoardItem, maxChars = 120): string | null {
  if (!TERMINAL_STATUSES.has(item.status)) return null
  const text = item.remark?.trim()
  if (!text) return null
  if (text.length <= maxChars) return text
  return `${text.slice(0, maxChars)}…`
}

/** Single-line row label: `title` plus terminal `remark` when present. */
export function milestoneRowLabel(item: TaskBoardItem, maxChars = 120): string {
  const title = milestoneTitle(item)
  const remark = milestoneRemark(item, maxChars)
  return remark ? `${title} ${remark}` : title
}
