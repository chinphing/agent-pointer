import type { AgentTrace, ChatMessage } from '../../types/chat'
import { toolCallBaseName } from '../messageTooling'

/** Every narrow signature one spawn's rows publish, built in a single pass. */
export type ScopedSignals = {
  /** Live-line fingerprint (v-memo on running SubAgentFrame rows). Moves per chunk. */
  live: string
  /** ask_user-relevant signature (top-of-chat banner). Never moves on token writes. */
  askUser: string
  /** Shape of the spawn (rows / tool calls / nested traces). Never moves on token writes. */
  structure: string
}

/**
 * Single pass over one spawn's rows — plus the legacy trace session when present —
 * producing every signature the scoped store publishes.
 *
 * One loop on purpose: `refreshFingerprint` runs on every streamed chunk, so a
 * second (or third) scan over the same rows would multiply the per-token cost.
 *
 * `structure` deliberately omits text / payload lengths: it only changes when the
 * spawn's **shape** changes — rows added or removed, tool call ids / names / status,
 * nested trace ids / status / links — which is what the turn spawn plan depends on.
 */
export function computeScopedSignals(
  scoped: readonly ChatMessage[],
  legacySession?: AgentTrace['session']
): ScopedSignals {
  let toolSig = ''
  let textLen = 0
  let askUser = ''
  let structure = ''
  for (const msg of scoped) {
    textLen += (msg.content?.length ?? 0)
      + (msg.thoughts?.length ?? 0)
      + (msg.responseTextDraft?.length ?? 0)
      + (msg.reasoning?.length ?? 0)
      + (msg.rawContent?.length ?? 0)
      + (msg.toolNamePreview?.length ?? 0)
    // status/contentStreaming flag: round-boundary transitions (streaming→done)
    // must bump even when text lengths are unchanged.
    toolSig += msg.status === 'streaming' || msg.contentStreaming === true ? '~' : '.'
    structure += `${msg.id}:${msg.status}`
      + `:${(msg.anchorMessageId ?? '').trim()}`
      + `:${(msg.agentInstanceId ?? '').trim()}`
      + `:${(msg.traceId ?? '').trim()}|`
    for (const tc of msg.toolCalls ?? []) {
      toolSig += `${tc.id}:${tc.status}:${tc.result?.length ?? 0}:${tc.displayLabel?.length ?? 0}:${tc.displaySummary?.length ?? 0}:${tc.arguments?.length ?? 0};`
      structure += `${tc.id}:${toolCallBaseName(tc.name)}:${tc.status};`
      if (toolCallBaseName(tc.name) === 'ask_user') {
        askUser += `${msg.id}/${tc.id}:${tc.status}:${tc.arguments?.length ?? 0}:${tc.displaySummary?.length ?? 0};`
      }
    }
    for (const trace of msg.agentTrace ?? []) {
      structure += `#${trace.id}:${trace.status}`
        + `:${(trace.parentToolCallId ?? '').trim()}`
        + `:${(trace.parentTraceId ?? '').trim()}`
        + `:${(trace.agentInstanceId ?? '').trim()};`
    }
    structure += '\n'
  }
  const legacy = legacySession
  if (legacy) {
    textLen += (legacy.thoughts?.length ?? 0)
      + (legacy.responseTextDraft?.length ?? 0)
      + (legacy.reasoning?.length ?? 0)
      + (legacy.rawContent?.length ?? 0)
    toolSig += legacy.contentStreaming === true ? '~' : '.'
    for (const tc of legacy.toolCalls ?? []) {
      toolSig += `${tc.id}:${tc.status}:${tc.result?.length ?? 0}:${tc.displayLabel?.length ?? 0}:${tc.displaySummary?.length ?? 0}:${tc.arguments?.length ?? 0};`
      structure += `L${tc.id}:${toolCallBaseName(tc.name)}:${tc.status};`
    }
    structure += 'L|'
  }
  return { live: `${textLen}|${toolSig}`, askUser, structure }
}

/** Cheap live-line fingerprint for v-memo on running SubAgentFrame rows. */
export function computeSubAgentLiveFingerprint(
  scoped: readonly ChatMessage[],
  legacySession?: AgentTrace['session']
): string {
  return computeScopedSignals(scoped, legacySession).live
}

/**
 * Ask_user-relevant signature for one spawn's rows.
 *
 * The top-of-chat banner is the only consumer: it rescans its queue whenever this
 * changes. Only `ask_user` tool calls count, and only the fields the banner renders —
 * `status` plus `arguments` (the question / options, which stream in chunk by chunk)
 * and the `displaySummary` fallback. Row text, tool `result` growth and non-ask_user
 * tool traffic stay invisible, so a streaming row no longer rescans the banner.
 */
export function computeScopedAskUserSignature(scoped: readonly ChatMessage[]): string {
  return computeScopedSignals(scoped).askUser
}

/**
 * Structure signature for one spawn's rows.
 *
 * Consumer: the per-turn spawn plan cache in `MessageList` (`turnSpawnPlans`). It
 * replaced the old "read the live signal with an empty key" subscription, which
 * invalidated the whole list on **every** streamed chunk. Only shape changes move it.
 */
export function computeScopedStructureSignature(
  scoped: readonly ChatMessage[],
  legacySession?: AgentTrace['session']
): string {
  return computeScopedSignals(scoped, legacySession).structure
}
