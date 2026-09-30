import type { AgentTrace, ChatMessage } from '../../types/chat'
import { toolCallBaseName } from '../messageTooling'

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
      + (msg.rawContent?.length ?? 0)
      + (msg.toolNamePreview?.length ?? 0)
    // status/contentStreaming flag: round-boundary transitions (streaming→done)
    // must bump even when text lengths are unchanged.
    toolSig += msg.status === 'streaming' || msg.contentStreaming === true ? '~' : '.'
    for (const tc of msg.toolCalls ?? []) {
      toolSig += `${tc.id}:${tc.status}:${tc.result?.length ?? 0}:${tc.displayLabel?.length ?? 0}:${tc.displaySummary?.length ?? 0}:${tc.arguments?.length ?? 0};`
    }
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
    }
  }
  return `${textLen}|${toolSig}`
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
  let out = ''
  for (const msg of scoped) {
    for (const tc of msg.toolCalls ?? []) {
      if (toolCallBaseName(tc.name) !== 'ask_user') continue
      out += `${msg.id}/${tc.id}:${tc.status}:${tc.arguments?.length ?? 0}:${tc.displaySummary?.length ?? 0};`
    }
  }
  return out
}
