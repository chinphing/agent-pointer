export type WorkspacePreviewTabKind = 'file' | 'diff' | 'turn-diff'
export type WorkspaceTabCloseAction = 'close' | 'close-others' | 'close-right'

export function workspacePreviewTabId(
  kind: WorkspacePreviewTabKind,
  path: string,
  turnId?: string
): string {
  if (kind === 'turn-diff') return `turn-diff:${turnId ?? ''}:${path}`
  return `${kind}:${path}`
}

export function workspaceTabIdsToClose(
  ids: string[],
  targetId: string,
  action: WorkspaceTabCloseAction
): string[] {
  const targetIndex = ids.indexOf(targetId)
  if (targetIndex < 0) return []
  if (action === 'close') return [targetId]
  if (action === 'close-others') return ids.filter(id => id !== targetId)
  return ids.slice(targetIndex + 1)
}

export function workspaceActiveAfterClose(
  ids: string[],
  activeId: string,
  closingIds: string[],
  targetId: string,
  fallback: string
): string {
  if (!closingIds.includes(activeId)) return activeId
  const remaining = ids.filter(id => !closingIds.includes(id))
  if (!remaining.length) return fallback
  const targetIndex = ids.indexOf(targetId)
  const right = ids.slice(targetIndex + 1).find(id => remaining.includes(id))
  if (right) return right
  const left = [...ids.slice(0, Math.max(0, targetIndex))].reverse().find(id => remaining.includes(id))
  return left ?? remaining[remaining.length - 1]!
}

/** Drop turn-diff tabs that belong to another conversation after a session switch. */
export function filterPreviewTabsForConversation<T extends { kind: string; conversationId?: string }>(
  tabs: readonly T[],
  conversationId: string
): T[] {
  const id = conversationId.trim()
  return tabs.filter(tab => tab.kind !== 'turn-diff' || tab.conversationId === id)
}
