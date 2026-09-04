import { ref, shallowRef } from 'vue'
import type { AgentTrace, ChatMessage } from '../../types/chat'
import { computeSubAgentLiveFingerprint } from './liveFingerprint'
import { agentIdFromLookup, resolveSpawnId, taskIdFromLookup } from './spawnId'
import type {
  ScopedEvictReason,
  ScopedInstanceTranscript,
  SpawnId,
  SpawnLookup
} from './types'

type ConvScopedState = {
  bySpawn: Map<SpawnId, ScopedInstanceTranscript>
  messageIdToInstance: Map<string, SpawnId>
  rowById: Map<string, ChatMessage>
  instancesByAnchor: Map<string, Set<SpawnId>>
}

function emptyConvState(): ConvScopedState {
  return {
    bySpawn: new Map(),
    messageIdToInstance: new Map(),
    rowById: new Map(),
    instancesByAnchor: new Map()
  }
}

function sortRows(rows: ChatMessage[]): ChatMessage[] {
  return [...rows].sort((a, b) => (a.position ?? a.createdAt) - (b.position ?? b.createdAt))
}

function isScopedRow(msg: ChatMessage): boolean {
  return !!msg.anchorMessageId?.trim()
}

function emptyTranscript(lookup: SpawnLookup & { instanceId: SpawnId }): ScopedInstanceTranscript {
  return {
    instanceId: lookup.instanceId,
    anchorMessageId: lookup.anchorMessageId?.trim() ?? '',
    legacyTraceId: lookup.traceId?.trim() ?? '',
    taskId: taskIdFromLookup(lookup),
    agentId: agentIdFromLookup(lookup),
    rows: [],
    loadState: 'streaming',
    liveFingerprint: ''
  }
}

export function createConversationScopedStore() {
  const states = shallowRef(new Map<string, ConvScopedState>())
  const liveSignals = ref<Record<string, Record<string, string>>>({})
  /** Bumps only when a spawn is added or removed — not on token writes. */
  const membership = ref<Record<string, number>>({})
  const version = ref(0)

  function bumpMembership(convId: string) {
    const id = convId.trim()
    if (!id) return
    membership.value[id] = (membership.value[id] ?? 0) + 1
  }

  function getState(convId: string): ConvScopedState | undefined {
    return states.value.get(convId.trim())
  }

  function ensureState(convId: string): ConvScopedState {
    const id = convId.trim()
    let state = states.value.get(id)
    if (state) return state
    state = emptyConvState()
    const next = new Map(states.value)
    next.set(id, state)
    states.value = next
    return state
  }

  function indexRow(state: ConvScopedState, spawnId: SpawnId, row: ChatMessage) {
    state.messageIdToInstance.set(row.id, spawnId)
    state.rowById.set(row.id, row)
    const anchor = row.anchorMessageId?.trim()
    if (!anchor) return
    let set = state.instancesByAnchor.get(anchor)
    if (!set) {
      set = new Set()
      state.instancesByAnchor.set(anchor, set)
    }
    set.add(spawnId)
  }

  function unindexTranscript(state: ConvScopedState, transcript: ScopedInstanceTranscript) {
    for (const row of transcript.rows) {
      if (state.messageIdToInstance.get(row.id) === transcript.instanceId) {
        state.messageIdToInstance.delete(row.id)
        state.rowById.delete(row.id)
      }
    }
  }

  function publishLive(convId: string, transcript: ScopedInstanceTranscript) {
    const id = convId.trim()
    const fp = transcript.liveFingerprint
    let convMap = liveSignals.value[id]
    if (!convMap) {
      convMap = {}
      liveSignals.value[id] = convMap
    }
    const instanceId = transcript.instanceId
    const legacy = transcript.legacyTraceId
    if (
      convMap[instanceId] === fp
      && (!legacy || legacy === instanceId || convMap[legacy] === fp)
    ) {
      return
    }
    convMap[instanceId] = fp
    if (legacy && legacy !== instanceId) convMap[legacy] = fp
  }

  function refreshFingerprint(
    convId: string,
    transcript: ScopedInstanceTranscript,
    legacySession?: AgentTrace['session']
  ) {
    transcript.liveFingerprint = computeSubAgentLiveFingerprint(transcript.rows, legacySession)
    publishLive(convId, transcript)
  }

  function upsertTranscript(
    convId: string,
    lookup: SpawnLookup,
    opts?: { createState?: ScopedInstanceTranscript['loadState'] }
  ): ScopedInstanceTranscript | null {
    const spawnId = resolveSpawnId(lookup)
    if (!spawnId) {
      console.warn('[conversationScoped] missing spawn id', convId, lookup)
      return null
    }
    const state = ensureState(convId)
    let transcript = state.bySpawn.get(spawnId)
    if (!transcript) {
      transcript = emptyTranscript({ ...lookup, instanceId: spawnId })
      if (opts?.createState) transcript.loadState = opts.createState
      state.bySpawn.set(spawnId, transcript)
      const anchor = transcript.anchorMessageId
      if (anchor) {
        let set = state.instancesByAnchor.get(anchor)
        if (!set) {
          set = new Set()
          state.instancesByAnchor.set(anchor, set)
        }
        set.add(spawnId)
      }
      bumpMembership(convId)
    } else {
      if (lookup.anchorMessageId?.trim()) transcript.anchorMessageId = lookup.anchorMessageId.trim()
      if (lookup.traceId?.trim()) transcript.legacyTraceId = lookup.traceId.trim()
      if (lookup.taskId?.trim()) transcript.taskId = lookup.taskId.trim()
      if (lookup.agentId?.trim()) transcript.agentId = lookup.agentId.trim()
    }
    return transcript
  }

  function ensureInstance(
    convId: string,
    lookup: SpawnLookup,
    firstRow?: ChatMessage
  ): ChatMessage | undefined {
    const transcript = upsertTranscript(convId, lookup, { createState: 'streaming' })
    if (!transcript) return undefined
    if (transcript.loadState === 'evicted') transcript.loadState = 'streaming'
    if (!firstRow) {
      refreshFingerprint(convId, transcript)
      return undefined
    }
    const existing = transcript.rows.find(m => m.id === firstRow.id)
    if (existing) return existing
    transcript.rows = sortRows([...transcript.rows, firstRow])
    indexRow(ensureState(convId), transcript.instanceId, firstRow)
    refreshFingerprint(convId, transcript)
    return firstRow
  }

  function assignLoaded(convId: string, lookup: SpawnLookup, rows: ChatMessage[]): void {
    const scoped = rows.filter(isScopedRow)
    const transcript = upsertTranscript(convId, lookup, { createState: 'loaded' })
    if (!transcript) return
    const state = ensureState(convId)
    unindexTranscript(state, transcript)
    const keepLive = transcript.loadState === 'streaming' && transcript.rows.length > 0
    const byId = new Map(transcript.rows.map(m => [m.id, m]))
    for (const row of scoped) {
      if (!byId.has(row.id)) byId.set(row.id, row)
    }
    transcript.rows = sortRows([...byId.values()])
    if (!keepLive) transcript.loadState = 'loaded'
    for (const row of transcript.rows) indexRow(state, transcript.instanceId, row)
    refreshFingerprint(convId, transcript)
  }

  function ingestRows(convId: string, rows: readonly ChatMessage[]): ChatMessage[] {
    const scoped = rows.filter(isScopedRow)
    if (!scoped.length) return []
    const groups = new Map<SpawnId, { lookup: SpawnLookup; rows: ChatMessage[] }>()
    for (const row of scoped) {
      const lookup: SpawnLookup = {
        agentInstanceId: row.agentInstanceId,
        anchorMessageId: row.anchorMessageId,
        traceId: row.traceId,
        taskId: row.taskId
      }
      const spawnId = resolveSpawnId(lookup)
      if (!spawnId) continue
      const group = groups.get(spawnId)
      if (group) group.rows.push(row)
      else groups.set(spawnId, { lookup, rows: [row] })
    }
    for (const { lookup, rows: groupRows } of groups.values()) {
      assignLoaded(convId, lookup, groupRows)
    }
    return scoped
  }

  function takeScopedFromMessages(convId: string, messages: ChatMessage[]): ChatMessage[] {
    const scoped = ingestRows(convId, messages)
    if (!scoped.length) return messages
    const ids = new Set(scoped.map(m => m.id))
    return messages.filter(m => !ids.has(m.id))
  }

  function findRow(convId: string, messageId: string): ChatMessage | undefined {
    const id = messageId.trim()
    if (!id) return undefined
    const state = getState(convId)
    if (!state) return undefined
    return state.rowById.get(id)
  }

  function findRowInAny(messageId: string): { convId: string; row: ChatMessage } | undefined {
    const id = messageId.trim()
    if (!id) return undefined
    for (const [convId, state] of states.value) {
      const row = state.rowById.get(id)
      if (row) return { convId, row }
    }
    return undefined
  }

  function getTranscript(convId: string, lookup: SpawnLookup): ScopedInstanceTranscript | undefined {
    const spawnId = resolveSpawnId(lookup)
    if (!spawnId) return undefined
    const direct = getState(convId)?.bySpawn.get(spawnId)
    if (direct) return direct
    const state = getState(convId)
    if (!state) return undefined
    const trace = lookup.traceId?.trim()
    const anchor = lookup.anchorMessageId?.trim()
    if (!trace) return undefined
    let fallback: ScopedInstanceTranscript | undefined
    for (const transcript of state.bySpawn.values()) {
      const byLegacy = transcript.legacyTraceId === trace
      const byInstance = transcript.instanceId === trace
      if (!byLegacy && !byInstance) continue
      if (anchor && transcript.anchorMessageId !== anchor) continue
      if (lookup.agentInstanceId?.trim() && transcript.instanceId !== lookup.agentInstanceId.trim()) {
        continue
      }
      fallback = transcript
    }
    return fallback
  }

  function getRows(convId: string, lookup: SpawnLookup): ChatMessage[] {
    const transcript = getTranscript(convId, lookup)
    if (!transcript || transcript.loadState === 'evicted') return []
    return transcript.rows
  }

  function hasLoadedRows(convId: string, lookup: SpawnLookup): boolean {
    const transcript = getTranscript(convId, lookup)
    if (!transcript) return false
    if (transcript.loadState === 'evicted') return false
    return transcript.rows.length > 0
  }

  function listRows(convId: string): ChatMessage[] {
    const state = getState(convId)
    if (!state) return []
    const out: ChatMessage[] = []
    for (const transcript of state.bySpawn.values()) {
      if (transcript.loadState === 'evicted') continue
      out.push(...transcript.rows)
    }
    return out
  }

  function listUnpersistedRows(
    convId: string,
    persistedIds?: ReadonlySet<string>
  ): ChatMessage[] {
    const state = getState(convId)
    if (!state) return []
    const out: ChatMessage[] = []
    for (const transcript of state.bySpawn.values()) {
      if (transcript.loadState === 'evicted') continue
      for (const row of transcript.rows) {
        if (row.status === 'pending') continue
        if (persistedIds?.has(row.id)) continue
        out.push(row)
      }
    }
    return out
  }

  function listSpawnIdsForAnchors(convId: string, anchorIds: readonly string[]): SpawnId[] {
    const state = getState(convId)
    if (!state) return []
    const out: SpawnId[] = []
    const seen = new Set<string>()
    for (const raw of anchorIds) {
      const anchor = raw.trim()
      if (!anchor) continue
      const spawnIds = state.instancesByAnchor.get(anchor)
      if (!spawnIds) continue
      for (const spawnId of spawnIds) {
        if (seen.has(spawnId)) continue
        seen.add(spawnId)
        out.push(spawnId)
      }
    }
    return out
  }

  function collectRowsForAnchors(convId: string, anchorIds: readonly string[]): ChatMessage[] {
    const state = getState(convId)
    if (!state) return []
    const out: ChatMessage[] = []
    const seen = new Set<string>()
    for (const raw of anchorIds) {
      const anchor = raw.trim()
      if (!anchor) continue
      const spawnIds = state.instancesByAnchor.get(anchor)
      if (!spawnIds) continue
      for (const spawnId of spawnIds) {
        const transcript = state.bySpawn.get(spawnId)
        if (!transcript || transcript.loadState === 'evicted') continue
        for (const row of transcript.rows) {
          if (seen.has(row.id)) continue
          seen.add(row.id)
          out.push(row)
        }
      }
    }
    return out
  }

  function touchRow(
    convId: string,
    messageId: string,
    legacySession?: AgentTrace['session']
  ): ChatMessage | undefined {
    const row = findRow(convId, messageId)
    if (!row) return undefined
    const spawnId = getState(convId)?.messageIdToInstance.get(messageId.trim())
    const transcript = spawnId ? getState(convId)?.bySpawn.get(spawnId) : undefined
    if (transcript) {
      refreshFingerprint(convId, transcript, legacySession)
    }
    return row
  }

  function touchLookup(
    convId: string,
    lookup: SpawnLookup,
    legacySession?: AgentTrace['session']
  ) {
    const transcript = getTranscript(convId, lookup)
    if (!transcript) return
    refreshFingerprint(convId, transcript, legacySession)
  }

  function evictInstance(convId: string, lookup: SpawnLookup, reason: ScopedEvictReason): void {
    const transcript = getTranscript(convId, lookup)
    if (!transcript) return
    const state = getState(convId)
    if (!state) return
    const childRowIds = transcript.rows.map(row => row.id)
    const spawnId = transcript.instanceId
    const legacy = transcript.legacyTraceId
    const anchor = transcript.anchorMessageId
    unindexTranscript(state, transcript)
    state.bySpawn.delete(spawnId)
    const anchored = state.instancesByAnchor.get(anchor)
    if (anchored) {
      anchored.delete(spawnId)
      if (anchored.size === 0) state.instancesByAnchor.delete(anchor)
    }
    const id = convId.trim()
    const prevLive = liveSignals.value[id]
    if (prevLive) {
      delete prevLive[spawnId]
      if (legacy) delete prevLive[legacy]
    }
    bumpMembership(convId)
    console.info('[conversationScoped] evicted instance', convId, spawnId, reason)
    for (const rowId of childRowIds) {
      evictForAnchor(convId, rowId)
    }
  }

  function evictForAnchor(convId: string, anchorMessageId: string): void {
    const state = getState(convId)
    if (!state) return
    const spawnIds = [...(state.instancesByAnchor.get(anchorMessageId.trim()) ?? [])]
    for (const spawnId of spawnIds) {
      evictInstance(convId, { agentInstanceId: spawnId }, 'trim')
    }
    state.instancesByAnchor.delete(anchorMessageId.trim())
  }

  function evictForRemovedMessages(convId: string, removed: readonly ChatMessage[]): void {
    for (const msg of removed) {
      evictForAnchor(convId, msg.id)
    }
  }

  function clearConversation(convId: string): void {
    const id = convId.trim()
    if (!states.value.has(id)) return
    const next = new Map(states.value)
    next.delete(id)
    states.value = next
    if (id in liveSignals.value) {
      delete liveSignals.value[id]
    }
    if (id in membership.value) {
      delete membership.value[id]
    }
    version.value += 1
  }

  function clearAll(): void {
    states.value = new Map()
    liveSignals.value = {}
    membership.value = {}
    version.value += 1
  }

  function getLiveSignal(convId: string | null | undefined, signalKey: string): string {
    const id = convId?.trim()
    if (!id) return ''
    return liveSignals.value[id]?.[signalKey.trim()] ?? ''
  }

  function getMembershipSignal(convId: string | null | undefined): number {
    const id = convId?.trim()
    if (!id) return 0
    return membership.value[id] ?? 0
  }

  return {
    liveSignals,
    version,
    resolveSpawnId,
    ensureInstance,
    assignLoaded,
    ingestRows,
    takeScopedFromMessages,
    findRow,
    findRowInAny,
    getTranscript,
    getRows,
    hasLoadedRows,
    listRows,
    listUnpersistedRows,
    listSpawnIdsForAnchors,
    collectRowsForAnchors,
    touchRow,
    touchLookup,
    evictInstance,
    evictForAnchor,
    evictForRemovedMessages,
    clearConversation,
    clearAll,
    getLiveSignal,
    getMembershipSignal
  }
}

export type ConversationScopedStore = ReturnType<typeof createConversationScopedStore>

let sharedStore: ConversationScopedStore | null = null

export function useConversationScopedStore(): ConversationScopedStore {
  if (!sharedStore) sharedStore = createConversationScopedStore()
  return sharedStore
}

/** Test-only: drop the process-wide singleton. */
export function resetConversationScopedStoreForTests(): void {
  sharedStore?.clearAll()
  sharedStore = null
}
