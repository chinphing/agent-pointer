import type { Ref } from 'vue'
import type { ChatMessage, TaskBoardDocument } from '../../types/chat'
import { findLastRealUserMessage } from '../../lib/messageContext'
import { hasTaskBoardContent } from '../../lib/taskBoard'

export const TASK_BOARD_SUB_SEP = '\u{1f}ptr_sub_agent\u{1f}'
export const TASK_BOARD_MAIN_TURN_SEP = '\u{1f}ptr_main_turn\u{1f}'
const TASK_BOARD_DEBOUNCE_MS = 300

export interface ConversationTaskBoardState {
  parentByStoreKey: Record<string, TaskBoardDocument>
  parentBindings: Record<string, string>
  /** Child store key → lead assistant message id. */
  childBindings: Record<string, string>
  activeParentStoreKey: string | null
  childrenByParentStoreKey: Record<string, Record<string, TaskBoardDocument>>
}

export function emptyTaskBoardEntry(): ConversationTaskBoardState {
  return {
    parentByStoreKey: {},
    parentBindings: {},
    childBindings: {},
    activeParentStoreKey: null,
    childrenByParentStoreKey: {}
  }
}

export function isTaskBoardTerminal(status: string | undefined): boolean {
  const s = (status ?? '').trim()
  return s === 'completed' || s === 'failed'
}

export function anchorFromMainTaskBoardStoreKey(storeKey: string): string | null {
  const idx = storeKey.indexOf(TASK_BOARD_MAIN_TURN_SEP)
  if (idx < 0) return null
  const msgId = storeKey.slice(idx + TASK_BOARD_MAIN_TURN_SEP.length).trim()
  return msgId || null
}

export function childStoreKey(parentStoreKey: string, taskId: string): string {
  return `${parentStoreKey.trim()}${TASK_BOARD_SUB_SEP}${taskId.trim()}`
}

/** Pure in-memory task board merge (no timers / network). */
export function applyTaskBoardDocumentToEntry(
  entry: ConversationTaskBoardState,
  convId: string,
  storeKey: string,
  doc: TaskBoardDocument,
  anchorMessageId: string | undefined,
  messagesForAnchorFallback: ChatMessage[] | undefined
): void {
  if (storeKey === convId || !storeKey.includes(TASK_BOARD_SUB_SEP)) {
    if (hasTaskBoardContent(doc)) {
      entry.parentByStoreKey[storeKey] = doc
    } else {
      delete entry.parentByStoreKey[storeKey]
    }
    let resolvedAnchor =
      anchorMessageId?.trim() ||
      entry.parentBindings[storeKey] ||
      anchorFromMainTaskBoardStoreKey(storeKey) ||
      ''
    if (!resolvedAnchor && messagesForAnchorFallback) {
      resolvedAnchor = findLastRealUserMessage(messagesForAnchorFallback)?.id || ''
    }
    if (resolvedAnchor) {
      entry.parentBindings[storeKey] = resolvedAnchor
    }
    if (hasTaskBoardContent(doc) && !isTaskBoardTerminal(doc.meta?.status)) {
      entry.activeParentStoreKey = storeKey
    } else if (entry.activeParentStoreKey === storeKey && isTaskBoardTerminal(doc.meta?.status)) {
      entry.activeParentStoreKey = null
    }
    return
  }

  const splitIdx = storeKey.lastIndexOf(TASK_BOARD_SUB_SEP)
  if (splitIdx <= 0) return
  const parentStoreKey = storeKey.slice(0, splitIdx).trim()
  const taskId = storeKey.slice(splitIdx + TASK_BOARD_SUB_SEP.length).trim()
  if (!parentStoreKey || !taskId) return
  const group = entry.childrenByParentStoreKey[parentStoreKey] ?? {}
  if (hasTaskBoardContent(doc)) {
    group[taskId] = doc
  } else {
    delete group[taskId]
    delete entry.childBindings[storeKey]
  }
  if (Object.keys(group).length > 0) {
    entry.childrenByParentStoreKey[parentStoreKey] = group
  } else {
    delete entry.childrenByParentStoreKey[parentStoreKey]
  }
  const anchor = anchorMessageId?.trim()
  if (anchor) {
    entry.childBindings[storeKey] = anchor
  }
}

export interface TaskBoardManager {
  applyTaskBoardDocumentDebounced(
    convId: string,
    storeKey: string,
    doc: TaskBoardDocument,
    anchorMessageId?: string
  ): void
  refreshTaskBoard(
    conversationId: string,
    taskId?: string,
    anchorMessageId?: string
  ): Promise<void>
  parentBoardsBoundToMessage(
    convId: string | null,
    messageId: string
  ): Array<{ storeKey: string; document: TaskBoardDocument; isActive: boolean }>
  taskBoardForConversation(convId: string | null): ConversationTaskBoardState | null
  childBoardsForParent(
    convId: string | null,
    parentStoreKey: string
  ): Record<string, TaskBoardDocument>
  lookupChildTaskBoard(
    convId: string | null,
    taskId: string,
    messageId?: string
  ): TaskBoardDocument | null
}

export function createTaskBoardManager(deps: {
  taskBoards: Ref<Record<string, ConversationTaskBoardState>>
  getMessages: (convId: string) => ChatMessage[] | undefined
  fetchSnapshot: (conversationId: string, taskId?: string) => Promise<unknown>
  showChildBoards: () => boolean
}): TaskBoardManager {
  const debounceTimers = new Map<string, ReturnType<typeof setTimeout>>()

  function ensureEntry(convId: string): ConversationTaskBoardState {
    if (!deps.taskBoards.value[convId]) {
      deps.taskBoards.value[convId] = emptyTaskBoardEntry()
    }
    return deps.taskBoards.value[convId]
  }

  function applyTaskBoardDocument(
    convId: string,
    storeKey: string,
    doc: TaskBoardDocument,
    anchorMessageId?: string
  ) {
    const entry = ensureEntry(convId)
    applyTaskBoardDocumentToEntry(
      entry,
      convId,
      storeKey,
      doc,
      anchorMessageId,
      deps.getMessages(convId)
    )
  }

  function applyTaskBoardDocumentDebounced(
    convId: string,
    storeKey: string,
    doc: TaskBoardDocument,
    anchorMessageId?: string
  ) {
    const timerKey = `${convId}\u{0}|${storeKey}`
    const prev = debounceTimers.get(timerKey)
    if (prev != null) window.clearTimeout(prev)
    debounceTimers.set(
      timerKey,
      window.setTimeout(() => {
        debounceTimers.delete(timerKey)
        applyTaskBoardDocument(convId, storeKey, doc, anchorMessageId)
      }, TASK_BOARD_DEBOUNCE_MS)
    )
  }

  async function refreshTaskBoard(
    conversationId: string,
    taskId?: string,
    anchorMessageId?: string
  ) {
    try {
      const doc = (await deps.fetchSnapshot(conversationId, taskId)) as TaskBoardDocument
      const inferredStoreKey =
        typeof doc.task_id === 'string' && doc.task_id.startsWith('tb_')
          ? doc.task_id.slice(3)
          : ''
      const storeKey = taskId?.trim()
        ? `${deps.taskBoards.value[conversationId]?.activeParentStoreKey || conversationId}${TASK_BOARD_SUB_SEP}${taskId.trim()}`
        : inferredStoreKey ||
          deps.taskBoards.value[conversationId]?.activeParentStoreKey ||
          conversationId
      applyTaskBoardDocument(conversationId, storeKey, doc, anchorMessageId)
    } catch (e) {
      console.warn('[task board] snapshot failed', e)
    }
  }

  function parentBoardsBoundToMessage(convId: string | null, messageId: string) {
    if (!convId) return []
    const entry = deps.taskBoards.value[convId]
    if (!entry) return []
    const list: Array<{ storeKey: string; document: TaskBoardDocument; isActive: boolean }> = []
    for (const [storeKey, anchor] of Object.entries(entry.parentBindings)) {
      if (anchor !== messageId) continue
      const document = entry.parentByStoreKey[storeKey]
      if (!document || !hasTaskBoardContent(document)) continue
      list.push({
        storeKey,
        document,
        isActive: entry.activeParentStoreKey === storeKey
      })
    }
    return list
  }

  function taskBoardForConversation(convId: string | null): ConversationTaskBoardState | null {
    if (!convId) return null
    return deps.taskBoards.value[convId] ?? null
  }

  function childBoardsForParent(convId: string | null, parentStoreKey: string) {
    if (!convId || !deps.showChildBoards()) return {}
    const entry = deps.taskBoards.value[convId]
    if (!entry) return {}
    return entry.childrenByParentStoreKey[parentStoreKey] ?? {}
  }

  function lookupChildTaskBoard(
    convId: string | null,
    taskId: string,
    messageId?: string
  ): TaskBoardDocument | null {
    if (!convId || !taskId.trim()) return null
    const entry = deps.taskBoards.value[convId]
    if (!entry) return null
    const tid = taskId.trim()
    for (const [parentStoreKey, group] of Object.entries(entry.childrenByParentStoreKey)) {
      const doc = group[tid]
      if (!doc || !hasTaskBoardContent(doc)) continue
      if (messageId?.trim()) {
        const storeKey = childStoreKey(parentStoreKey, tid)
        const bound = entry.childBindings?.[storeKey]
        if (bound && bound !== messageId.trim()) continue
      }
      return doc
    }
    return null
  }

  return {
    applyTaskBoardDocumentDebounced,
    refreshTaskBoard,
    parentBoardsBoundToMessage,
    taskBoardForConversation,
    childBoardsForParent,
    lookupChildTaskBoard
  }
}
