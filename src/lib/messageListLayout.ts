import type { ChatMessage, TaskBoardDocument, ToolCall } from '../types/chat'
import {
  assistantDisplayKind,
  assistantHasDeliverableContent,
  isEphemeralDesktopNoticeMessage,
  isToolOnlyAssistantMessage
} from './assistantMessageKind'
import { isCompressionSummaryMessage } from './compressionMessage'
import {
  buildConversationTurns,
  type ConversationTurn
} from './conversationTurns'
import { isInteractiveToolCall } from './messageTooling'
import { isScopedSubMessage } from './subAgentMessages'
import { isRealUserTaskMessage, isToolRunContinuityGlue } from './threadLayoutGlue'

export type ToolRunGroup = { id: string; toolCalls: ToolCall[]; message: ChatMessage }

export type ToolRunItem =
  | { kind: 'tools'; group: ToolRunGroup }
  | { kind: 'glue'; message: ChatMessage }

export type FlatEntry =
  | {
      type: 'message'
      message: ChatMessage
      trailingToolGroups?: ToolRunGroup[]
      compact?: boolean
      /** Collapsed projection: render reply body only (no process tools / thoughts). */
      contentOnly?: boolean
    }
  | { type: 'tool_run'; items: ToolRunItem[] }
  | {
      type: 'task_board'
      anchorMessageId: string
      storeKey: string
      document: TaskBoardDocument
      isActive: boolean
    }

export type MessageListBoardBinding = {
  storeKey: string
  document: TaskBoardDocument
  isActive: boolean
}

export type FlattenDeps = {
  boardsForMessage: (messageId: string) => MessageListBoardBinding[]
  visibleToolCallsFor: (message: ChatMessage) => ToolCall[]
  shouldShowGlue: (message: ChatMessage) => boolean
}

export type MessageListLayoutCache = {
  conversationId: string
  fingerprints: string[]
  entries: FlatEntry[]
  turns: ConversationTurn<FlatEntry>[]
}

export type MessageListLayoutResult = {
  entries: FlatEntry[]
  turns: ConversationTurn<FlatEntry>[]
  cache: MessageListLayoutCache
  reusablePrefixTurns: number
}

export type TurnSegment = {
  id: string
  start: number
  end: number
}

type LayoutKind = 'skip' | 'notice' | 'tool_only' | 'glue' | 'message'

function layoutKind(message: ChatMessage): LayoutKind {
  if (isScopedSubMessage(message)) return 'skip'
  if (isEphemeralDesktopNoticeMessage(message)) return 'notice'
  if (isToolOnlyAssistantMessage(message)) return 'tool_only'
  if (isToolRunContinuityGlue(message)) return 'glue'
  return 'message'
}

function canAttachTrailingTools(message: ChatMessage): boolean {
  return (
    message.role === 'assistant'
    && !isToolOnlyAssistantMessage(message)
    && assistantDisplayKind(message) === 'model'
  )
}

function isTaskBoardTerminal(status: string | undefined): boolean {
  const s = (status ?? '').trim()
  return s === 'completed' || s === 'failed'
}

/** Split the raw message list into user-anchored segments (same boundaries as turns). */
export function splitMessageTurnSegments(messages: readonly ChatMessage[]): TurnSegment[] {
  const segments: TurnSegment[] = []
  let current: TurnSegment | null = null

  for (let i = 0; i < messages.length; i++) {
    const message = messages[i]!
    // Skip screen-inject / empty-response retry injects — they are wire-only.
    const isUserAnchor = isRealUserTaskMessage(message) && !isScopedSubMessage(message)
    if (isUserAnchor) {
      if (current) segments.push(current)
      current = { id: message.id, start: i, end: i + 1 }
      continue
    }
    if (!current) {
      current = { id: `prelude-${message.id}`, start: i, end: i + 1 }
    } else {
      current.end = i + 1
    }
  }
  if (current) segments.push(current)
  return segments
}

/**
 * Structure signature for layout rebuild caching.
 * Intentionally omits content / reasoning / tool payloads.
 */
export function messageStructureFingerprint(
  message: ChatMessage,
  deps: FlattenDeps
): string | null {
  const kind = layoutKind(message)
  if (kind === 'skip') return null

  const tools = kind === 'tool_only' || kind === 'message'
    ? deps.visibleToolCallsFor(message).map(tc => `${tc.id}:${tc.status}`).join(',')
    : ''
  const glue = kind === 'glue' ? (deps.shouldShowGlue(message) ? '1' : '0') : '0'
  const boards = deps
    .boardsForMessage(message.id)
    .map(b => `${b.storeKey}:${b.document.version}:${b.document.meta?.status ?? ''}:${b.isActive ? '1' : '0'}`)
    .join(',')
  const summary = isCompressionSummaryMessage(message) ? '1' : '0'
  const delivery =
    message.role === 'assistant'
    && assistantHasDeliverableContent(message)
    && kind === 'message'
      ? '1'
      : '0'

  return [
    message.id,
    message.role,
    message.status,
    kind,
    `tools:${tools}`,
    `glue:${glue}`,
    `boards:${boards}`,
    `summary:${summary}`,
    `delivery:${delivery}`
  ].join('|')
}

export function turnSegmentFingerprint(
  messages: readonly ChatMessage[],
  start: number,
  end: number,
  deps: FlattenDeps
): string {
  const parts: string[] = []
  for (let i = start; i < end; i++) {
    const fp = messageStructureFingerprint(messages[i]!, deps)
    if (fp) parts.push(fp)
  }
  return parts.join('||')
}

export function matchingCompletedPrefixCount(
  conversationId: string,
  fingerprints: readonly string[],
  cache: MessageListLayoutCache | null
): number {
  if (!cache || cache.conversationId !== conversationId) return 0
  // Always recompute the tail turn — that is where streaming lands.
  const maxReusable = Math.max(0, fingerprints.length - 1)
  let n = 0
  while (
    n < maxReusable
    && n < cache.fingerprints.length
    && n < cache.turns.length
    && fingerprints[n] === cache.fingerprints[n]
    && cache.turns[n]!.state !== 'active'
  ) {
    n += 1
  }
  return n
}

function toolRunHasTools(items: ToolRunItem[]): boolean {
  return items.some(i => i.kind === 'tools')
}

/** Flatten messages into thread display entries (message / tool_run / task_board). */
export function flattenConversationMessages(
  messages: readonly ChatMessage[],
  deps: FlattenDeps
): FlatEntry[] {
  const entries: FlatEntry[] = []
  let toolRunItems: ToolRunItem[] = []

  function flushToolRun() {
    if (toolRunItems.length === 0) return
    if (!toolRunHasTools(toolRunItems)) {
      for (const item of toolRunItems) {
        if (item.kind === 'glue' && deps.shouldShowGlue(item.message)) {
          entries.push({ type: 'message', message: item.message, compact: true })
        }
      }
      toolRunItems = []
      return
    }
    const last = entries[entries.length - 1]
    const groups = toolRunItems
      .filter((i): i is { kind: 'tools'; group: ToolRunGroup } => i.kind === 'tools')
      .map(i => i.group)
    const hasGlue = toolRunItems.some(
      i => i.kind === 'glue' && deps.shouldShowGlue(i.message)
    )
    if (last?.type === 'message' && canAttachTrailingTools(last.message) && !hasGlue) {
      last.trailingToolGroups = [...(last.trailingToolGroups ?? []), ...groups]
    } else {
      entries.push({ type: 'tool_run', items: [...toolRunItems] })
    }
    toolRunItems = []
  }

  for (const message of messages) {
    const kind = layoutKind(message)
    if (kind === 'skip') continue
    if (kind === 'notice') {
      flushToolRun()
      entries.push({ type: 'message', message })
    } else if (kind === 'tool_only') {
      const visible = deps.visibleToolCallsFor(message)
      if (visible.length > 0) {
        toolRunItems.push({
          kind: 'tools',
          group: {
            id: message.id,
            toolCalls: message.toolCalls ?? [],
            message
          }
        })
      }
    } else if (kind === 'glue') {
      if (toolRunHasTools(toolRunItems)) {
        if (deps.shouldShowGlue(message)) {
          toolRunItems.push({ kind: 'glue', message })
        }
      } else if (deps.shouldShowGlue(message)) {
        flushToolRun()
        entries.push({ type: 'message', message, compact: true })
      }
    } else {
      flushToolRun()
      entries.push({ type: 'message', message })
    }

    const boards = deps.boardsForMessage(message.id)
    for (const board of boards) {
      flushToolRun()
      entries.push({
        type: 'task_board',
        anchorMessageId: message.id,
        storeKey: board.storeKey,
        document: board.document,
        isActive: board.isActive
      })
    }
  }
  flushToolRun()
  return entries
}

function messageByIdMap(messages: readonly ChatMessage[]): Map<string, ChatMessage> {
  const map = new Map<string, ChatMessage>()
  for (const message of messages) map.set(message.id, message)
  return map
}

function rebindMessage(message: ChatMessage, byId: Map<string, ChatMessage>): ChatMessage {
  return byId.get(message.id) ?? message
}

function rebindFlatEntry(
  entry: FlatEntry,
  byId: Map<string, ChatMessage>,
  deps: FlattenDeps
): FlatEntry {
  if (entry.type === 'message') {
    const message = rebindMessage(entry.message, byId)
    const trailingToolGroups = entry.trailingToolGroups?.map(group => {
      const groupMessage = rebindMessage(group.message, byId)
      return {
        id: group.id,
        message: groupMessage,
        toolCalls: groupMessage.toolCalls ?? group.toolCalls
      }
    })
    return {
      type: 'message',
      message,
      ...(trailingToolGroups ? { trailingToolGroups } : {}),
      ...(entry.compact ? { compact: true } : {}),
      ...(entry.contentOnly ? { contentOnly: true } : {})
    }
  }
  if (entry.type === 'tool_run') {
    return {
      type: 'tool_run',
      items: entry.items.map(item => {
        if (item.kind === 'glue') {
          return { kind: 'glue' as const, message: rebindMessage(item.message, byId) }
        }
        const message = rebindMessage(item.group.message, byId)
        return {
          kind: 'tools' as const,
          group: {
            id: item.group.id,
            message,
            toolCalls: message.toolCalls ?? item.group.toolCalls
          }
        }
      })
    }
  }
  const boards = deps.boardsForMessage(entry.anchorMessageId)
  const board = boards.find(b => b.storeKey === entry.storeKey)
  if (!board) return entry
  return {
    type: 'task_board',
    anchorMessageId: entry.anchorMessageId,
    storeKey: board.storeKey,
    document: board.document,
    isActive: board.isActive
  }
}

function rebindTurn(
  turn: ConversationTurn<FlatEntry>,
  byId: Map<string, ChatMessage>,
  deps: FlattenDeps
): ConversationTurn<FlatEntry> {
  const entries = turn.entries.map(entry => rebindFlatEntry(entry, byId, deps))
  const rebound = new Map(turn.entries.map((entry, index) => [entry, entries[index]!]))
  return {
    ...turn,
    entries,
    collapsedEntries: turn.collapsedEntries.map(entry => rebound.get(entry) ?? rebindFlatEntry(entry, byId, deps))
  }
}

export function entryContainsMessageId(entry: FlatEntry, messageId: string): boolean {
  const id = messageId.trim()
  if (!id) return false
  if (entry.type === 'message') return entry.message.id === id
  if (entry.type === 'tool_run') {
    return entry.items.some(item =>
      item.kind === 'tools'
        ? item.group.message.id === id || item.group.id === id
        : item.message.id === id
    )
  }
  return entry.anchorMessageId === id
}

export function entryKey(entry: FlatEntry): string {
  if (entry.type === 'message') return `message-${entry.message.id}`
  if (entry.type === 'tool_run') {
    return `tool-run-${entry.items
      .map(item => item.kind === 'tools' ? item.group.id : item.message.id)
      .join('-')}`
  }
  return `task-board-${entry.storeKey}-${entry.anchorMessageId}`
}

function entryMessageStatuses(entry: FlatEntry): ChatMessage[] {
  if (entry.type === 'message') return [entry.message]
  if (entry.type === 'tool_run') {
    return entry.items.map(item => item.kind === 'tools' ? item.group.message : item.message)
  }
  return []
}

function entryHasStatus(entry: FlatEntry, statuses: ChatMessage['status'][]): boolean {
  return entryMessageStatuses(entry).some(message => statuses.includes(message.status))
}

function entryHasRunningTool(entry: FlatEntry): boolean {
  return entryMessageStatuses(entry).some(message =>
    message.toolCalls?.some(tool => tool.status === 'running' || tool.status === 'pending')
  )
}

function entryIsSummary(entry: FlatEntry): boolean {
  // Task boards are progress chrome, not process to hide — keep running and
  // terminal boards in the collapsed projection (sticky also needs the inline
  // mount). Only compression summaries use the same keep path among messages.
  if (entry.type === 'task_board') return true
  return entry.type === 'message' && isCompressionSummaryMessage(entry.message)
}

function entryIsDelivery(entry: FlatEntry): boolean {
  return entry.type === 'message'
    && entry.message.role === 'assistant'
    && assistantHasDeliverableContent(entry.message)
    && !isEphemeralDesktopNoticeMessage(entry.message)
    && !isToolRunContinuityGlue(entry.message)
}

function toolCallsAreInteractive(toolCalls: readonly ToolCall[] | undefined): boolean {
  return (toolCalls ?? []).some(isInteractiveToolCall)
}

function entryIsInteractive(entry: FlatEntry): boolean {
  if (entry.type === 'tool_run') {
    return entry.items.some(
      item => item.kind === 'tools' && toolCallsAreInteractive(item.group.toolCalls)
    )
  }
  if (entry.type === 'message') {
    if (toolCallsAreInteractive(entry.message.toolCalls)) return true
    return (entry.trailingToolGroups ?? []).some(group => toolCallsAreInteractive(group.toolCalls))
  }
  return false
}

function filterInteractiveToolGroups(
  groups: ToolRunGroup[] | undefined
): ToolRunGroup[] | undefined {
  if (!groups?.length) return undefined
  const next = groups
    .map(group => ({
      ...group,
      toolCalls: group.toolCalls.filter(isInteractiveToolCall)
    }))
    .filter(group => group.toolCalls.length > 0)
  return next.length > 0 ? next : undefined
}

/** True when projecting this kept entry will hide process UI (not just interactive tools). */
function entryContributesHiddenProcess(entry: FlatEntry): boolean {
  if (entry.type === 'tool_run') {
    return entry.items.some(
      item => item.kind === 'tools'
        && item.group.toolCalls.some(tc => !isInteractiveToolCall(tc))
    )
  }
  if (entry.type !== 'message') return false
  if ((entry.trailingToolGroups ?? []).some(group =>
    group.toolCalls.some(tc => !isInteractiveToolCall(tc))
  )) {
    return true
  }
  if ((entry.message.toolCalls ?? []).some(tc => !isInteractiveToolCall(tc))) return true
  if ((entry.message.agentTrace?.length ?? 0) > 0) return true
  if (entry.message.thoughts?.trim() || entry.message.reasoning?.trim()) return true
  return false
}

/**
 * Collapsed projection: keep reply body only. Strip process tools attached to
 * the delivery entry (trailingToolGroups / non-interactive toolCalls) so they
 * do not leak under the final content. Interactive tools stay.
 */
function projectCollapsedEntry(entry: FlatEntry): FlatEntry {
  if (entry.type === 'tool_run') {
    return {
      type: 'tool_run',
      items: entry.items
        .map(item => {
          if (item.kind !== 'tools') return item
          const toolCalls = item.group.toolCalls.filter(isInteractiveToolCall)
          if (toolCalls.length === 0) return null
          return {
            kind: 'tools' as const,
            group: { ...item.group, toolCalls }
          }
        })
        .filter((item): item is ToolRunItem => item != null)
    }
  }
  if (entry.type !== 'message') return entry
  if (entry.message.role !== 'assistant') return entry
  if (entry.compact || isCompressionSummaryMessage(entry.message)) return entry
  if (isEphemeralDesktopNoticeMessage(entry.message)) return entry

  const trailingToolGroups = filterInteractiveToolGroups(entry.trailingToolGroups)
  return {
    type: 'message',
    message: entry.message,
    contentOnly: true,
    ...(trailingToolGroups ? { trailingToolGroups } : {}),
    ...(entry.compact ? { compact: true } : {})
  }
}

function projectCollapsedTurn(turn: ConversationTurn<FlatEntry>): ConversationTurn<FlatEntry> {
  let extraHidden = 0
  const collapsedEntries = turn.collapsedEntries.map(entry => {
    if (entryContributesHiddenProcess(entry)) extraHidden += 1
    return projectCollapsedEntry(entry)
  })
  // Drop tool_run shells that lost every tool after interactive filtering.
  const filtered = collapsedEntries.filter(entry =>
    entry.type !== 'tool_run' || entry.items.some(item => item.kind === 'tools')
  )
  return {
    ...turn,
    collapsedEntries: filtered,
    hiddenCount: turn.hiddenCount + extraHidden + (turn.collapsedEntries.length - filtered.length)
  }
}

function findActiveBoard(entries: readonly FlatEntry[]): Extract<FlatEntry, { type: 'task_board' }> | null {
  return entries.find(
    (entry): entry is Extract<FlatEntry, { type: 'task_board' }> =>
      entry.type === 'task_board' && entry.isActive && !isTaskBoardTerminal(entry.document.meta?.status)
  ) ?? null
}

function buildTurnsForEntries(
  entries: readonly FlatEntry[],
  activeBoard: Extract<FlatEntry, { type: 'task_board' }> | null,
  collapseActiveTurns: boolean
): ConversationTurn<FlatEntry>[] {
  return buildConversationTurns(entries, {
    key: entryKey,
    userMessageId: entry => entry.type === 'message' && entry.message.role === 'user'
      ? entry.message.id
      : null,
    isActive: entry => entryHasStatus(entry, ['pending', 'streaming'])
      || entryHasRunningTool(entry)
      || (!!activeBoard && entryContainsMessageId(entry, activeBoard.anchorMessageId)),
    isFailed: entry => entryHasStatus(entry, ['error'])
      || (entry.type === 'task_board' && entry.document.meta?.status === 'failed'),
    isCancelled: entry => entryHasStatus(entry, ['cancelled'])
      || (entry.type === 'task_board' && entry.document.meta?.status === 'cancelled'),
    isSummary: entryIsSummary,
    isDelivery: entryIsDelivery,
    isInteractive: entryIsInteractive
  }, {
    collapseActiveTurns,
    omitDeliveryWhileActive: true
  }).map(turn => {
    // Early-return full turns share the same array ref — leave them untouched.
    if (turn.collapsedEntries === turn.entries) return turn
    return projectCollapsedTurn(turn)
  })
}

/**
 * Build flat entries + conversation turns, reusing completed prefix turns when
 * their structure fingerprints still match the previous layout.
 */
export function buildMessageListLayout(options: {
  conversationId: string | null
  messages: readonly ChatMessage[]
  deps: FlattenDeps
  cache: MessageListLayoutCache | null
  /** 「默认收缩执行过程」：进行中回合也可折叠并显示耗时条。 */
  collapseActiveTurns?: boolean
}): MessageListLayoutResult {
  const conversationId = options.conversationId?.trim() || ''
  const messages = options.messages
  const deps = options.deps
  const collapseActiveTurns = options.collapseActiveTurns === true

  if (!conversationId || messages.length === 0) {
    const emptyCache: MessageListLayoutCache = {
      conversationId,
      fingerprints: [],
      entries: [],
      turns: []
    }
    return { entries: [], turns: [], cache: emptyCache, reusablePrefixTurns: 0 }
  }

  const segments = splitMessageTurnSegments(messages)
  const fingerprints = segments.map(segment =>
    turnSegmentFingerprint(messages, segment.start, segment.end, deps)
  )
  const reusablePrefixTurns = matchingCompletedPrefixCount(
    conversationId,
    fingerprints,
    options.cache
  )

  const byId = messageByIdMap(messages)
  let entries: FlatEntry[]
  let turns: ConversationTurn<FlatEntry>[]

  if (reusablePrefixTurns <= 0 || !options.cache) {
    if (options.cache && segments.length > 1) {
      console.info(
        '[messageListLayout] full rebuild (prefix cache miss)',
        conversationId,
        'segments',
        segments.length
      )
    }
    entries = flattenConversationMessages(messages, deps)
    const activeBoard = findActiveBoard(entries)
    turns = buildTurnsForEntries(entries, activeBoard, collapseActiveTurns)
  } else {
    const prefixEntryCount = options.cache.turns
      .slice(0, reusablePrefixTurns)
      .reduce((sum, turn) => sum + turn.entries.length, 0)
    const prefixEntries = options.cache.entries
      .slice(0, prefixEntryCount)
      .map(entry => rebindFlatEntry(entry, byId, deps))
    const suffixStart = segments[reusablePrefixTurns]!.start
    const suffixEntries = flattenConversationMessages(messages.slice(suffixStart), deps)
    entries = [...prefixEntries, ...suffixEntries]

    const activeBoard = findActiveBoard(entries)
    const prefixTurns = options.cache.turns
      .slice(0, reusablePrefixTurns)
      .map(turn => rebindTurn(turn, byId, deps))
    const suffixTurns = buildTurnsForEntries(suffixEntries, activeBoard, collapseActiveTurns)
    turns = [...prefixTurns, ...suffixTurns]
  }

  const cache: MessageListLayoutCache = {
    conversationId,
    fingerprints,
    entries,
    turns
  }
  return { entries, turns, cache, reusablePrefixTurns }
}
