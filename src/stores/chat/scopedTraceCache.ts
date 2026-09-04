import { ref, shallowRef } from 'vue'
import type { AgentTrace, ChatMessage } from '../../types/chat'
import { isScopedSubMessage } from '../../lib/subAgentMessages'
import {
  computeSubAgentLiveFingerprint,
  emptyScopedTraceIndex,
  getScopedMessagesFromIndex,
  rebuildScopedTraceIndex,
  registerScopedMessageInIndex,
  type ScopedTraceIndex
} from '../../lib/scopedTraceIndex'

let sharedCache: ReturnType<typeof createScopedTraceCache> | null = null

/** Process-wide scoped trace index (one chat store instance). */
export function useScopedTraceCache() {
  if (!sharedCache) sharedCache = createScopedTraceCache()
  return sharedCache
}

/** Per-conversation scoped trace index + incremental live fingerprints (P0 sub-agent UI perf). */
export function createScopedTraceCache() {
  const indexes = shallowRef(new Map<string, ScopedTraceIndex>())
  const liveSignals = ref<Record<string, Record<string, string>>>({})

  function setIndex(convId: string, index: ScopedTraceIndex) {
    const next = new Map(indexes.value)
    next.set(convId, index)
    indexes.value = next
  }

  function rebuild(convId: string, messages: readonly ChatMessage[]) {
    setIndex(convId, rebuildScopedTraceIndex(messages))
  }

  function clearConv(convId: string) {
    const next = new Map(indexes.value)
    next.delete(convId)
    indexes.value = next
    if (!(convId in liveSignals.value)) return
    const copy = { ...liveSignals.value }
    delete copy[convId]
    liveSignals.value = copy
  }

  function clearAll() {
    indexes.value = new Map()
    liveSignals.value = {}
  }

  function ensureIndex(convId: string, messages: readonly ChatMessage[]): ScopedTraceIndex {
    const existing = indexes.value.get(convId)
    if (existing) return existing
    const built = rebuildScopedTraceIndex(messages)
    setIndex(convId, built)
    return built
  }

  function getMessages(
    convId: string,
    messages: readonly ChatMessage[],
    anchorMessageId: string,
    traceId: string,
    agentInstanceId?: string
  ): ChatMessage[] {
    const index = ensureIndex(convId, messages)
    return getScopedMessagesFromIndex(index, anchorMessageId, traceId, agentInstanceId)
  }

  function publishLiveSignal(convId: string, traceId: string, fingerprint: string) {
    const tid = traceId.trim()
    if (!tid) return
    const prev = liveSignals.value[convId]?.[tid]
    if (prev === fingerprint) return
    liveSignals.value = {
      ...liveSignals.value,
      [convId]: {
        ...(liveSignals.value[convId] ?? {}),
        [tid]: fingerprint
      }
    }
  }

  function touchScopedMessage(convId: string, msg: ChatMessage) {
    if (!isScopedSubMessage(msg)) return
    const index = indexes.value.get(convId) ?? emptyScopedTraceIndex()
    const had = index.messageKeys.has(msg.id)
    registerScopedMessageInIndex(index, msg)
    if (!had || !indexes.value.has(convId)) {
      setIndex(convId, index)
    }
    const scoped = getScopedMessagesFromIndex(
      index,
      msg.anchorMessageId!,
      msg.traceId!,
      msg.agentInstanceId
    )
    publishLiveSignal(convId, msg.traceId!, computeSubAgentLiveFingerprint(scoped))
  }

  function touchLegacySession(
    convId: string,
    anchorMessageId: string,
    traceId: string,
    agentInstanceId: string | undefined,
    messages: readonly ChatMessage[],
    legacySession?: AgentTrace['session']
  ) {
    const scoped = getMessages(convId, messages, anchorMessageId, traceId, agentInstanceId)
    publishLiveSignal(convId, traceId, computeSubAgentLiveFingerprint(scoped, legacySession))
  }

  function getLiveSignal(convId: string | null | undefined, traceId: string): string {
    const id = convId?.trim()
    if (!id) return ''
    return liveSignals.value[id]?.[traceId.trim()] ?? ''
  }

  return {
    indexes,
    liveSignals,
    rebuild,
    clearConv,
    clearAll,
    getMessages,
    touchScopedMessage,
    touchLegacySession,
    getLiveSignal
  }
}

export type ScopedTraceCache = ReturnType<typeof createScopedTraceCache>
