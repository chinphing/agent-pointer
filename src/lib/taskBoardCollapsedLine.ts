import type { TaskBoardDocument } from '../types/chat'

function rowLabel(item: {
  id: string
  title?: string
  validate_results?: string[] | null
}): string {
  const title = item.title?.trim()
  if (title) return title
  const results = item.validate_results ?? []
  const last = results[results.length - 1]?.trim()
  if (last) {
    return last.length > 48 ? `${last.slice(0, 48)}…` : last
  }
  return `#${item.id}`
}

/** One-line collapsed summary for task board (matches TaskBoardPanel summary). */
export function taskBoardCollapsedLine(document: TaskBoardDocument | null | undefined): string | null {
  if (!document?.board?.length && !document?.meta?.goal?.trim()) return null

  const goal = document.meta?.goal?.trim() || '任务板'
  const items = document.board ?? []
  const doneCount = items.filter(i => i.status === 'done').length
  const total = items.length
  const progress = total > 0 ? `${doneCount}/${total}` : '0/0'

  const inProgress = items.find(i => i.status === 'in_progress')
  if (inProgress) {
    return `${goal} · ${progress} · ${rowLabel(inProgress)}`
  }
  return `${goal} · ${progress}`
}
