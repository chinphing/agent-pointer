import type { Ref } from 'vue'
import type { ChatMessage, TaskBoardDocument } from '../../types/chat'
import { findLastRealUserMessage } from '../../lib/messageContext'
import { hasTaskBoardContent } from '../../lib/taskBoard'
import { subTaskIdFromTraceId } from '../../lib/subAgentStats'

export const TASK_BOARD_SUB_SEP = '\u{1f}ptr_sub_agent\u{1f}'
export const TASK_BOARD_MAIN_TURN_SEP = '\u{1f}ptr_main_turn\u{1f}'
const TASK_BOARD_DEBOUNCE_MS = 300

export interface ConversationTaskBoardState {
  parentByStoreKey: Record<string, TaskBoardDocument>
  parentBindings: Record<string, string>
  /** Child store key → sub-agent trace id (`{taskId}:{agentId}`). */
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
  activeParentBoardDocument(
    convId: string | null,
    messageId?: string | null
  ): TaskBoardDocument | null
  activeParentBoardBinding(
    convId: string | null,
    messageId?: string | null
  ): { storeKey: string; document: TaskBoardDocument; isActive: boolean } | null
  compactTaskBoardDocument(
    convId: string | null,
    messageId: string | null | undefined,
    computerSubTaskId?: string | null,
    computerTraceId?: string | null
  ): TaskBoardDocument | null
  parentBoardsBoundToMessage(
    convId: string | null,
    messageId: string
  ): Array<{ storeKey: string; document: TaskBoardDocument; isActive: boolean }>
  childBoardBindingForTrace(
    convId: string | null,
    traceId: string,
    legacyLeadMessageId?: string | null
  ): { storeKey: string; document: TaskBoardDocument; isActive: boolean } | null
  taskBoardForConversation(convId: string | null): ConversationTaskBoardState | null
  childBoardsForParent(
    convId: string | null,
    parentStoreKey: string
  ): Record<string, TaskBoardDocument>
  lookupChildTaskBoard(
    convId: string | null,
    traceId: string,
    legacyLeadMessageId?: string | null
  ): TaskBoardDocument | null
}

function findStoreKeyForParentDocument(
  entry: ConversationTaskBoardState,
  document: TaskBoardDocument
): string | null {
  for (const [storeKey, doc] of Object.entries(entry.parentByStoreKey)) {
    if (doc === document) return storeKey
  }
  return null
}

export function resolveActiveParentBoardDocument(
  entry: ConversationTaskBoardState | null | undefined,
  messageId?: string | null
): TaskBoardDocument | null {
  if (!entry) return null
  const mid = messageId?.trim()
  if (mid) {
    for (const [storeKey, anchor] of Object.entries(entry.parentBindings)) {
      if (anchor !== mid) continue
      const document = entry.parentByStoreKey[storeKey]
      if (document && hasTaskBoardContent(document)) return document
    }
  }
  const activeKey = entry.activeParentStoreKey
  if (activeKey) {
    const document = entry.parentByStoreKey[activeKey]
    if (document && hasTaskBoardContent(document)) return document
  }
  for (const document of Object.values(entry.parentByStoreKey)) {
    if (document && hasTaskBoardContent(document) && !isTaskBoardTerminal(document.meta?.status)) {
      return document
    }
  }
  return null
}

export function resolveActiveParentBoardBinding(
  entry: ConversationTaskBoardState | null | undefined,
  messageId?: string | null
): { storeKey: string; document: TaskBoardDocument; isActive: boolean } | null {
  if (!entry) return null
  const document = resolveActiveParentBoardDocument(entry, messageId)
  if (!document) return null
  const storeKey =
    (entry.activeParentStoreKey &&
    entry.parentByStoreKey[entry.activeParentStoreKey] === document
      ? entry.activeParentStoreKey
      : null) ?? findStoreKeyForParentDocument(entry, document)
  if (!storeKey) return null
  return {
    storeKey,
    document,
    isActive: entry.activeParentStoreKey === storeKey
  }
}

/** Child board for a sub-agent trace (anchor is trace id; optional legacy lead message id). */
export function resolveChildTaskBoardDocument(
  entry: ConversationTaskBoardState | null | undefined,
  taskId: string,
  traceId?: string | null,
  legacyLeadMessageId?: string | null
): TaskBoardDocument | null {
  if (!entry || !taskId.trim()) return null
  const tid = taskId.trim()
  for (const [parentStoreKey, group] of Object.entries(entry.childrenByParentStoreKey)) {
    const document = group[tid]
    if (!document || !hasTaskBoardContent(document)) continue
    const storeKey = childStoreKey(parentStoreKey, tid)
    const bound = entry.childBindings?.[storeKey]
    if (bound) {
      const trace = traceId?.trim()
      const legacy = legacyLeadMessageId?.trim()
      const matches =
        (!!trace && bound === trace)
        || (!!legacy && bound === legacy)
      if (!matches) continue
    }
    return document
  }
  return null
}

function childStoreKeyForTask(
  entry: ConversationTaskBoardState,
  taskId: string
): string | null {
  const tid = taskId.trim()
  if (!tid) return null
  for (const [parentStoreKey, group] of Object.entries(entry.childrenByParentStoreKey)) {
    const document = group[tid]
    if (document && hasTaskBoardContent(document)) {
      return childStoreKey(parentStoreKey, tid)
    }
  }
  return null
}

/** Child task board binding for one delegated trace (MessageList / SubAgentFrame UI). */
export function childBoardBindingForTrace(
  entry: ConversationTaskBoardState | null | undefined,
  traceId: string,
  legacyLeadMessageId?: string | null
): { storeKey: string; document: TaskBoardDocument; isActive: boolean } | null {
  if (!entry || !traceId.trim()) return null
  const taskId = subTaskIdFromTraceId(traceId)
  if (!taskId) return null
  const document = resolveChildTaskBoardDocument(
    entry,
    taskId,
    traceId.trim(),
    legacyLeadMessageId
  )
  if (!document) return null
  const storeKey = childStoreKeyForTask(entry, taskId)
  if (!storeKey) return null
  return {
    storeKey,
    document,
    isActive: !isTaskBoardTerminal(document.meta?.status)
  }
}

/**
 * Task board document for the computer compact bar.
 * - Lead computer (no sub trace): main/parent board.
 * - Delegated computer sub-agent: child board when present, else parent.
 */
export function resolveCompactTaskBoardDocument(
  entry: ConversationTaskBoardState | null | undefined,
  messageId: string | null | undefined,
  /** Supervisor sub-task id only (not lead trace id `computer`). */
  computerSubTaskId?: string | null,
  /** Full delegated trace id (`{taskId}:{agentId}`) when known. */
  computerTraceId?: string | null
): TaskBoardDocument | null {
  if (!entry) return null
  const mid = messageId?.trim() || null
  const subTaskId = computerSubTaskId?.trim()
  const traceId = computerTraceId?.trim()
  const child = traceId
    ? resolveChildTaskBoardDocument(entry, subTaskIdFromTraceId(traceId), traceId, mid)
    : subTaskId && mid
      ? resolveChildTaskBoardDocument(entry, subTaskId, null, mid)
      : null
  const parent = resolveActiveParentBoardDocument(entry, mid)
  if (child && !isTaskBoardTerminal(child.meta?.status)) return child
  if (parent) return parent
  return child
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
      if (!hasTaskBoardContent(doc)) {
        console.warn('[task board] snapshot empty, keeping cached board', conversationId, taskId)
        return
      }
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

  /** Active parent board for compact dock bar (anchor is usually the user turn id, not assistant). */
  function activeParentBoardDocument(convId: string | null, messageId?: string | null) {
    if (!convId) return null
    return resolveActiveParentBoardDocument(deps.taskBoards.value[convId], messageId)
  }

  function activeParentBoardBinding(convId: string | null, messageId?: string | null) {
    if (!convId) return null
    return resolveActiveParentBoardBinding(deps.taskBoards.value[convId], messageId)
  }

  function compactTaskBoardDocument(
    convId: string | null,
    messageId: string | null | undefined,
    computerSubTaskId?: string | null,
    computerTraceId?: string | null
  ) {
    if (!convId) return null
    return resolveCompactTaskBoardDocument(
      deps.taskBoards.value[convId],
      messageId,
      computerSubTaskId,
      computerTraceId
    )
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

  function childBoardBindingForTraceForConv(
    convId: string | null,
    traceId: string,
    legacyLeadMessageId?: string | null
  ) {
    if (!convId) return null
    return childBoardBindingForTrace(
      deps.taskBoards.value[convId],
      traceId,
      legacyLeadMessageId
    )
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
    traceId: string,
    legacyLeadMessageId?: string | null
  ): TaskBoardDocument | null {
    if (!convId || !traceId.trim()) return null
    return childBoardBindingForTrace(
      deps.taskBoards.value[convId],
      traceId,
      legacyLeadMessageId
    )?.document ?? null
  }

  return {
    applyTaskBoardDocumentDebounced,
    refreshTaskBoard,
    activeParentBoardDocument,
    activeParentBoardBinding,
    compactTaskBoardDocument,
    parentBoardsBoundToMessage,
    childBoardBindingForTrace: childBoardBindingForTraceForConv,
    taskBoardForConversation,
    childBoardsForParent,
    lookupChildTaskBoard
  }
}
