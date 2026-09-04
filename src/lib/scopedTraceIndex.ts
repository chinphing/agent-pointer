import type { AgentTrace, ChatMessage } from '../types/chat'
import { isScopedSubMessage } from './subAgentMessages'

/** Bucket key: anchor + trace (instance filtered at read time). */
export function scopedTraceBucketKey(anchorMessageId: string, traceId: string): string {
  const anchor = anchorMessageId.trim()
  const trace = traceId.trim()
  return `${anchor}\0${trace}`
}

export type ScopedTraceIndex = {
  buckets: Map<string, ChatMessage[]>
  messageKeys: Map<string, string>
}

export function emptyScopedTraceIndex(): ScopedTraceIndex {
  return { buckets: new Map(), messageKeys: new Map() }
}

export function rebuildScopedTraceIndex(messages: readonly ChatMessage[]): ScopedTraceIndex {
  const index = emptyScopedTraceIndex()
  for (const msg of messages) {
    if (!isScopedSubMessage(msg)) continue
    registerScopedMessageInIndex(index, msg)
  }
  return index
}

/** Register a scoped row (append once per id). In-place content/tool mutations need no re-register. */
export function registerScopedMessageInIndex(index: ScopedTraceIndex, msg: ChatMessage): void {
  if (!isScopedSubMessage(msg)) return
  const key = scopedTraceBucketKey(msg.anchorMessageId!, msg.traceId!)
  const prevKey = index.messageKeys.get(msg.id)
  if (prevKey === key) return
  if (prevKey) {
    const prevBucket = index.buckets.get(prevKey)
    if (prevBucket) {
      index.buckets.set(
        prevKey,
        prevBucket.filter(m => m.id !== msg.id)
      )
    }
  }
  let bucket = index.buckets.get(key)
  if (!bucket) {
    bucket = []
    index.buckets.set(key, bucket)
  }
  if (!bucket.some(m => m.id === msg.id)) {
    bucket.push(msg)
    bucket.sort((a, b) => a.createdAt - b.createdAt)
  }
  index.messageKeys.set(msg.id, key)
}

export function getScopedMessagesFromIndex(
  index: ScopedTraceIndex,
  anchorMessageId: string,
  traceId: string,
  agentInstanceId?: string
): ChatMessage[] {
  const key = scopedTraceBucketKey(anchorMessageId, traceId)
  const bucket = index.buckets.get(key)
  if (!bucket?.length) return []
  const instance = agentInstanceId?.trim()
  if (!instance) return bucket
  return bucket.filter(m => m.agentInstanceId?.trim() === instance)
}

/** Cheap live-line fingerprint for v-memo on running SubAgentFrame rows. */
export function computeSubAgentLiveFingerprint(
  scoped: readonly ChatMessage[],
  legacySession?: AgentTrace['session']
): string {
  let toolSig = ''
  let textLen = 0
  for (const msg of scoped) {
    textLen += (msg.content?.length ?? 0)
      + (msg.thoughts?.length ?? 0)
      + (msg.responseTextDraft?.length ?? 0)
      + (msg.reasoning?.length ?? 0)
    for (const tc of msg.toolCalls ?? []) {
      toolSig += `${tc.id}:${tc.status}:${tc.result?.length ?? 0};`
    }
  }
  const legacy = legacySession
  if (legacy) {
    textLen += (legacy.thoughts?.length ?? 0)
      + (legacy.responseTextDraft?.length ?? 0)
      + (legacy.reasoning?.length ?? 0)
    for (const tc of legacy.toolCalls ?? []) {
      toolSig += `${tc.id}:${tc.status}:${tc.result?.length ?? 0};`
    }
  }
  return `${textLen}|${toolSig}`
}
