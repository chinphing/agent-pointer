import type { ChatMessage } from '../../types/chat'

/** One `run_subagent` spawn. Equals `agentInstanceId` when present. */
export type SpawnId = string

export type ScopedLoadState = 'streaming' | 'loaded' | 'evicted'

export type ScopedEvictReason = 'stub' | 'trim' | 'manual' | 'conversation'

/** Source-of-truth transcript for one sub-agent spawn. */
export interface ScopedInstanceTranscript {
  instanceId: SpawnId
  anchorMessageId: string
  /** SSE / AgentTrace.id during the traceId transition. */
  legacyTraceId: string
  taskId: string
  agentId: string
  rows: ChatMessage[]
  loadState: ScopedLoadState
  liveFingerprint: string
  /**
   * Ask_user-relevant signature of `rows` (see `computeScopedAskUserSignature`).
   * Cached so the store only bumps its narrow revision when it actually changes.
   */
  askUserSignature: string
}

export interface SpawnLookup {
  agentInstanceId?: string | null
  anchorMessageId?: string | null
  traceId?: string | null
  taskId?: string | null
  agentId?: string | null
}
