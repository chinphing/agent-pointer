import { EMPTY_CHAT_RETENTION, type ChatRetention } from './chatRetention'

/**
 * Chat-retention reading shape for the residency panel, and its zero value.
 *
 * Kept out of `lib/residencyProbe.ts` on purpose: that module reaches the whole
 * markdown pipeline (`markdownConfig` → `marked` + `dompurify`) for its cache
 * sizes, and `ChatView.vue` only needs the empty reading as a fallback. Importing
 * it from there pulled sanitizer + markdown parser into the first-paint chunk.
 */

/** Spawn transcripts retained for the open conversation, on top of `ChatRetention`. */
export interface ResidencyChatReading extends ChatRetention {
  scopedSpawns: number
}

/** Zero reading used when no chat source is available (tests, HUD without a host). */
export const EMPTY_RESIDENCY_CHAT: ResidencyChatReading = Object.freeze({
  ...EMPTY_CHAT_RETENTION,
  scopedSpawns: 0
})
