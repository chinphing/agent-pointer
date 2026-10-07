import { ref, shallowRef } from 'vue'
import type { AgentTrace, ChatMessage } from '../../types/chat'
import { computeScopedSignals, composeLiveFingerprint } from './liveFingerprint'
import { bump, setGauge } from '../../lib/renderPerf'
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
    liveFingerprint: '',
    textLen: 0,
    payloadLen: 0,
    liveShape: '',
    askUserSignature: '',
    structureSignature: ''
  }
}

export function createConversationScopedStore() {
  const states = shallowRef(new Map<string, ConvScopedState>())
  const liveSignals = ref<Record<string, Record<string, string>>>({})
  /** Bumps only when a spawn is added or removed — not on token writes. */
  const membership = ref<Record<string, number>>({})
  /**
   * Bumps only when a spawn's ask_user-relevant signature changes (see
   * `computeScopedAskUserSignature`) — row text growth and non-ask_user tool traffic
   * leave it untouched. The top-of-chat banner reads this instead of the live
   * fingerprint, which is replaced on every streamed chunk.
   */
  const askUserRevisions = ref<Record<string, number>>({})
  /**
   * Bumps only when a spawn's **shape** changes (see `computeScopedStructureSignature`):
   * rows added / removed, tool call ids / names / statuses, nested trace links. Row text
   * growth and tool payload growth leave it untouched. The per-turn spawn plan reads this
   * instead of the live fingerprint, which moves on every streamed chunk.
   */
  const structureRevisions = ref<Record<string, number>>({})
  /**
   * Per-**anchor** structure revisions: `convId → anchorMessageId → n`.
   *
   * `structureRevisions` above is a single counter per conversation, so it can only
   * answer "did *anything* move?". The per-turn spawn plans need "did anything **under
   * this turn** move?" — otherwise every spawn's shape event re-fingerprints every turn
   * (all lead messages + every scoped row). Each transcript already knows its anchor, so
   * the store can bump exactly that anchor's chain in O(depth) instead.
   *
   * The chain matters: a nested spawn's rows are anchored on a **row id** of its parent
   * worker, so a change two levels down must invalidate the turn that owns the root
   * anchor. `bumpAnchorStructureRevision` walks up via `messageIdToInstance` for exactly
   * that reason.
   */
  const anchorStructureRevisions = ref<Record<string, Record<string, number>>>({})
  /**
   * Bumped by shape events whose anchor cannot be resolved (a transcript created before
   * its anchor is known, a removed spawn whose anchor is already gone). Readers treat it
   * as "assume every turn changed" — conservative, and rare.
   */
  const anchorlessStructureEpoch = ref(0)
  const version = ref(0)
  /**
   * Every live scoped row id, across **all** conversations.
   *
   * Answering "is this messageId a sub-agent row?" has to be O(1) and global: the
   * `content` delta channel carries only a messageId and is enqueued before any
   * conversation lookup (see `lib/reasoningDeltaBatch.ts` lane routing). Maintained
   * next to the exact add/remove points of `messageIdToInstance` (`indexRow` /
   * `unindexTranscript`), so it cannot only grow.
   */
  const scopedRowIds = new Set<string>()

  function bumpMembership(convId: string) {
    const id = convId.trim()
    if (!id) return
    membership.value[id] = (membership.value[id] ?? 0) + 1
  }

  function bumpAskUserRevision(convId: string) {
    const id = convId.trim()
    if (!id) return
    askUserRevisions.value[id] = (askUserRevisions.value[id] ?? 0) + 1
  }

  function bumpStructureRevision(convId: string, anchorMessageId?: string) {
    const id = convId.trim()
    if (!id) return
    structureRevisions.value[id] = (structureRevisions.value[id] ?? 0) + 1
    // Per-anchor axis: what the per-turn spawn plans actually key on. Without an anchor
    // (or with one that resolves to no live transcript) fall back to the anchor-less
    // epoch, which every turn treats as a change.
    const anchor = anchorMessageId?.trim()
    if (!anchor) {
      anchorlessStructureEpoch.value += 1
      return
    }
    const chain = anchorChainOf(id, anchor)
    if (chain.length === 0) {
      anchorlessStructureEpoch.value += 1
      return
    }
    const conv = { ...(anchorStructureRevisions.value[id] ?? {}) }
    for (const key of chain) conv[key] = (conv[key] ?? 0) + 1
    anchorStructureRevisions.value = { ...anchorStructureRevisions.value, [id]: conv }
  }

  /**
   * `anchorId` and every ancestor anchor it hangs off — the root anchor is a turn message
   * id, deeper ones are scoped row ids. Depth is bounded by the render tree's nesting
   * (`scopedRowClosure` uses the same bound), and the `seen` guard keeps a corrupt
   * `anchorMessageId` cycle from looping forever.
   */
  function anchorChainOf(convId: string, anchorId: string): string[] {
    const state = getState(convId)
    const chain: string[] = []
    const seen = new Set<string>()
    let cursor = anchorId.trim()
    while (cursor && !seen.has(cursor)) {
      seen.add(cursor)
      chain.push(cursor)
      const spawnId = state?.messageIdToInstance.get(cursor)
      const transcript = spawnId ? state?.bySpawn.get(spawnId) : undefined
      cursor = transcript?.anchorMessageId?.trim() ?? ''
    }
    return chain
  }

  /** Shape revision of one anchor (0 = never changed). */
  function getAnchorStructureRevision(
    convId: string | null | undefined,
    anchorMessageId: string | null | undefined
  ): number {
    const id = convId?.trim()
    const anchor = anchorMessageId?.trim()
    if (!id || !anchor) return 0
    return anchorStructureRevisions.value[id]?.[anchor] ?? 0
  }

  /** Revision for shape events with no resolvable anchor (see the ref above). */
  function getAnchorlessStructureEpoch(): number {
    return anchorlessStructureEpoch.value
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
    if (isScopedRow(row)) scopedRowIds.add(row.id)
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
        scopedRowIds.delete(row.id)
      }
    }
  }

  function publishLive(convId: string, transcript: ScopedInstanceTranscript) {
    const id = convId.trim()
    const fp = transcript.liveFingerprint
    if (!liveSignals.value[id]) {
      // The conversation slot is written once, on first use: readers that started
      // before it existed re-run here, then track the inner spawn keys.
      liveSignals.value[id] = {}
    }
    // Always re-read through the ref: assigning a plain object stores the raw value,
    // and writing to that raw object would bypass reactivity.
    const convMap = liveSignals.value[id]!
    const instanceId = transcript.instanceId
    const legacy = transcript.legacyTraceId
    if (
      convMap[instanceId] === fp
      && (!legacy || legacy === instanceId || convMap[legacy] === fp)
    ) {
      return
    }
    // Precise inner writes — replacing the whole conversation slot would invalidate
    // every reader of every spawn in this conversation on each streamed chunk.
    convMap[instanceId] = fp
    if (legacy && legacy !== instanceId) convMap[legacy] = fp
  }

  function refreshFingerprint(
    convId: string,
    transcript: ScopedInstanceTranscript,
    legacySession?: AgentTrace['session']
  ) {
    // Full scan — **shape events only**: rows added / removed, tool status, trace links.
    // A text append must go through `touchTextDelta` (O(1)); this scan rebuilds ~32.7 KB
    // of strings per call (measured numbers live in `computeScopedSignals`).
    const signals = computeScopedSignals(transcript.rows, legacySession)
    transcript.liveFingerprint = signals.live
    transcript.textLen = signals.textLen
    transcript.payloadLen = signals.payloadLen
    transcript.liveShape = signals.liveShape
    // Rate reading for the HUD: how many full scans this chain costs per second, and how
    // many rows each one walks. `bump` / `setGauge` are no-ops while the HUD is off, so
    // the production hot path pays one boolean read.
    bump('call:scopedSignals')
    setGauge('scopedSignalRows', transcript.rows.length)
    publishLive(convId, transcript)
    // Narrow signal for the ask_user banner: row text growth and non-ask_user tool
    // traffic must not invalidate it (see computeScopedAskUserSignature).
    if (signals.askUser !== transcript.askUserSignature) {
      transcript.askUserSignature = signals.askUser
      bumpAskUserRevision(convId)
    }
    // Narrow signal for the per-turn spawn plans: shape changes only, never tokens.
    if (signals.structure !== transcript.structureSignature) {
      transcript.structureSignature = signals.structure
      bumpStructureRevision(convId, transcript.anchorMessageId)
    }
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
      // A new spawn is a shape change for the turn spawn plans even before its
      // first row lands (the transcript exists with an empty signature).
      bumpStructureRevision(convId, transcript.anchorMessageId)
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

  /**
   * O(1), conversation-agnostic answer to "is this a sub-agent (scoped) row id?".
   *
   * Registered as the `content` delta lane predicate (see `lib/reasoningDeltaBatch.ts`).
   * **Conservative by construction**: an unknown / empty id is `false`, i.e. a lead row
   * that must keep the fast lane — a miss here costs perf, never latency.
   */
  function isScopedMessageId(messageId: string | null | undefined): boolean {
    const id = messageId?.trim()
    if (!id) return false
    return scopedRowIds.has(id)
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

  /**
   * Live scoped rows for one conversation, counted without building the array
   * `listRows` returns. Read-only, for the HUD's residency panel: it is the row
   * count that tells a growing transcript apart from a leak (see `lib/residencyProbe.ts`).
   */
  function countRows(convId: string): number {
    const state = getState(convId)
    if (!state) return 0
    let count = 0
    for (const transcript of state.bySpawn.values()) {
      if (transcript.loadState === 'evicted') continue
      count += transcript.rows.length
    }
    return count
  }

  /** Spawn transcripts retained for one conversation, evicted ones included. */
  function countSpawns(convId: string): number {
    return getState(convId)?.bySpawn.size ?? 0
  }

  /**
   * Live spawn ids of one conversation, in insertion order. Read together with
   * `getLiveSignal(id)` / `getMembershipSignal(id)` by consumers that need "any
   * spawn moved" (conversation search) without depending on the outer slot.
   */
  function listSpawnIds(convId: string): SpawnId[] {
    const state = getState(convId)
    if (!state) return []
    return [...state.bySpawn.keys()]
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

  /**
   * A streamed chunk that only **appended** to one row — text (content / reasoning / raw)
   * or a tool-call payload (`arguments` / `result`).
   *
   * `askUser` and `structure` never move on token writes; `live` moves only through its two
   * length halves. So this is O(1): advance `textLen` / `payloadLen`, reuse the cached
   * shape, and skip the scan that used to rebuild several KB of strings on every chunk.
   *
   * `shapeChanged` is the caller's note that this chunk also flipped the row's streaming
   * flag: that is the one shape field a streamed append can touch, and `streaming → done`
   * must still bump even when no length moved.
   *
   * Callers that mutate anything else (tool call status / labels, row add / remove, traces)
   * must keep using `touchRow` / `touchLookup`, which always rescan.
   *
   * Measured: **0.13 µs / 13 bytes** against the scan it replaces (97.3 µs / 32.7 KB).
   * Guards: `store.test.ts` → `a text delta moves only the live length, never structure`,
   * and → `O(1) deltas stay byte-identical to a full rescan`.
   */
  function touchTextDelta(
    convId: string,
    messageId: string,
    deltaChars: number,
    opts?: { shapeChanged?: boolean; payloadDelta?: number; legacySession?: AgentTrace['session'] }
  ): ChatMessage | undefined {
    const row = findRow(convId, messageId)
    if (!row) return undefined
    const spawnId = getState(convId)?.messageIdToInstance.get(messageId.trim())
    const transcript = spawnId ? getState(convId)?.bySpawn.get(spawnId) : undefined
    if (!transcript) return row
    if (opts?.shapeChanged) {
      refreshFingerprint(convId, transcript, opts.legacySession)
      return row
    }
    transcript.textLen += deltaChars
    if (opts?.payloadDelta) transcript.payloadLen += opts.payloadDelta
    transcript.liveFingerprint = composeLiveFingerprint(
      transcript.textLen,
      transcript.payloadLen,
      transcript.liveShape
    )
    bump('call:scopedTextDelta')
    publishLive(convId, transcript)
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
    // Removing a spawn is a shape change for the turn spawn plans.
    bumpStructureRevision(convId, anchor)
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
    // Release this conversation's ids from the global scoped-row index too.
    const dropped = states.value.get(id)
    if (dropped) {
      for (const rowId of dropped.messageIdToInstance.keys()) scopedRowIds.delete(rowId)
    }
    const next = new Map(states.value)
    next.delete(id)
    states.value = next
    if (id in liveSignals.value) {
      delete liveSignals.value[id]
    }
    if (id in membership.value) {
      delete membership.value[id]
    }
    if (id in askUserRevisions.value) {
      delete askUserRevisions.value[id]
    }
    if (id in structureRevisions.value) {
      delete structureRevisions.value[id]
    }
    if (id in anchorStructureRevisions.value) {
      const next = { ...anchorStructureRevisions.value }
      delete next[id]
      anchorStructureRevisions.value = next
    }
    version.value += 1
  }

  function clearAll(): void {
    states.value = new Map()
    liveSignals.value = {}
    membership.value = {}
    askUserRevisions.value = {}
    structureRevisions.value = {}
    anchorStructureRevisions.value = {}
    anchorlessStructureEpoch.value += 1
    scopedRowIds.clear()
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

  /**
   * Narrow per-conversation revision for the ask_user banner: changes only when a
   * spawn's ask_user-relevant signature changes (never on token writes).
   */
  function getAskUserRevision(convId: string | null | undefined): number {
    const id = convId?.trim()
    if (!id) return 0
    return askUserRevisions.value[id] ?? 0
  }

  /**
   * Narrow per-conversation revision for the per-turn spawn plans: changes only when
   * some spawn's shape changes (rows / tool call ids / statuses / nested traces) or a
   * spawn is added / removed — never on token writes.
   */
  function getStructureRevision(convId: string | null | undefined): number {
    const id = convId?.trim()
    if (!id) return 0
    return structureRevisions.value[id] ?? 0
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
    isScopedMessageId,
    getTranscript,
    getRows,
    hasLoadedRows,
    listRows,
    countRows,
    countSpawns,
    listSpawnIds,
    listUnpersistedRows,
    listSpawnIdsForAnchors,
    collectRowsForAnchors,
    touchRow,
    touchTextDelta,
    touchLookup,
    evictInstance,
    evictForAnchor,
    evictForRemovedMessages,
    clearConversation,
    clearAll,
    getLiveSignal,
    getMembershipSignal,
    getAskUserRevision,
    getStructureRevision,
    getAnchorStructureRevision,
    getAnchorlessStructureEpoch,
    /** Lead-layer trace shape changes also invalidate the turn spawn plans. */
    bumpStructureRevision
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
