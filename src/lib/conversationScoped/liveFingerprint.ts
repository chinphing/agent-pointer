import type { AgentTrace, ChatMessage } from '../../types/chat'

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
