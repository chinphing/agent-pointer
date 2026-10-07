import type { AgentTrace, ChatMessage } from '../../types/chat'
import { toolCallBaseName } from '../messageTooling'

/** Every narrow signature one spawn's rows publish, built in a single pass. */
export type ScopedSignals = {
  /** Live-line fingerprint (v-memo on running SubAgentFrame rows). Moves per chunk. */
  live: string
  /**
   * Total text length across the rows. This is the **only** part of `live` a plain
   * text append moves, so the store can advance it in O(1) and skip the scan.
   */
  textLen: number
  /**
   * The shape half of `live`: streaming flags + every tool call's id / status / payload
   * lengths. Stays put on a plain text append; only a shape event rebuilds it.
   */
  liveShape: string
  /** ask_user-relevant signature (top-of-chat banner). Never moves on token writes. */
  askUser: string
  /** Shape of the spawn (rows / tool calls / nested traces). Never moves on token writes. */
  structure: string
}

/** `live` = the O(1)-incrementable length half + the shape half. */
export function composeLiveFingerprint(textLen: number, liveShape: string): string {
  return `${textLen}|${liveShape}`
}

/**
 * Single pass over one spawn's rows — plus the legacy trace session when present —
 * producing every signature the scoped store publishes.
 *
 * **Two invalidation axes, not one.** `live` carries `textLen`, so it moves on every
 * token; `askUser` / `structure` are documented above as *never moving on token writes*,
 * so rebuilding them per chunk is work whose result is thrown away. Per-token refreshes
 * therefore go through `touchTextDelta` (O(1)); only shape events land here.
 *
 * **The cost is bytes, not scan count.** This used to run per chunk on the text path and
 * measured **97.3 µs and 32,697 bytes of strings per call** (`live` 11,157B + `askUser`
 * 4,120B + `structure` 17,420B, at 140 rows / 290k chars). At 500–2000 chunks/s that is
 * 15.6–62.4 MB/s of garbage — which is why a real-device `sample` puts it on JSC's
 * *auxiliary* threads (`JSRopeString::resolveToBuffer`, `WTF::StringBuilder`, `bmalloc`,
 * `pas_`) with nothing to blame on the main thread: neither `ms:` nor `render:` can see
 * it, so "the HUD looks fine" is not evidence.
 *
 * One loop on purpose (below) is therefore **not** the fix, and was never sufficient: it
 * was already one loop and it was still the hot path. Do not "optimise" it further by
 * merging loops — stop calling it per chunk.
 *
 * `structure` deliberately omits text / payload lengths: it only changes when the
 * spawn's **shape** changes — rows added or removed, tool call ids / names / status,
 * nested trace ids / status / links — which is what the turn spawn plan depends on.
 *
 * **Check it**: HUD `call:scopedSignals` (scans/s) must stay ≈0 while a sub-agent streams;
 * if it climbs with the chunks, someone routed a text path back here. `call:scopedTextDelta`
 * carries the chunks and `scopedSignalRows` the rows per scan. Guard:
 * `store.test.ts` → `a text delta moves only the live length, never structure`.
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
  return {
    live: composeLiveFingerprint(textLen, toolSig),
    textLen,
    liveShape: toolSig,
    askUser,
    structure
  }
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
