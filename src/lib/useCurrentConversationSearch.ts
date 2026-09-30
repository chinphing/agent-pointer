import { computed, type ComputedRef, type Ref } from 'vue'
import type { ChatMessage } from '../types/chat'
import {
  EMPTY_SEARCH_IDS,
  EMPTY_SEARCH_MATCHES,
  findCurrentConversationMatches,
  type CurrentConversationSearchMatch
} from './currentConversationSearch'
import { useConversationScopedStore } from './conversationScoped'

/** Shared empty scoped-row list — same identity contract as {@link EMPTY_SEARCH_MATCHES}. */
const EMPTY_SCOPED_ROWS: readonly ChatMessage[] = Object.freeze([])

export type CurrentConversationSearchState = {
  /** True while a non-blank query is debounced in — the only state that reads the store. */
  active: ComputedRef<boolean>
  matches: ComputedRef<readonly CurrentConversationSearchMatch[]>
  matchMessageIds: ComputedRef<string[]>
  matchToolCallIds: ComputedRef<string[]>
  matchContentIds: ComputedRef<string[]>
}

/**
 * Current-conversation find box state (transcript + scoped sub-agent rows).
 *
 * While idle (blank query) every returned computed hands back the shared frozen
 * empties and never reads the transcript or the scoped store. That matters because
 * the scoped store's live signal is replaced on **every** streamed sub-agent chunk:
 * subscribing to it while the box is closed churned the search props, so the virtual
 * `MessageList` re-rendered per token with nothing to show.
 *
 * While active the search behaves exactly as before: scoped rows are read live and
 * `getMembershipSignal` / `getLiveSignal` are the reactive hooks for new rows and
 * row / tool status writes.
 */
export function useCurrentConversationSearch(options: {
  /** Debounced find-box query. */
  query: Ref<string>
  /** Current conversation id — only read while the search is active. */
  conversationId: () => string
  /** Lead transcript messages — only read while the search is active. */
  messages: () => readonly ChatMessage[]
}): CurrentConversationSearchState {
  const active = computed(() => options.query.value.trim().length > 0)

  const scopedRows = computed<readonly ChatMessage[]>(() => {
    if (!active.value) return EMPTY_SCOPED_ROWS
    const convId = options.conversationId().trim()
    if (!convId) return EMPTY_SCOPED_ROWS
    const store = useConversationScopedStore()
    // Scoped rows sit behind a shallowRef map — spawn membership and the live
    // fingerprint are what make row / tool status writes visible to this computed.
    void store.getMembershipSignal(convId)
    void store.getLiveSignal(convId, '')
    return store.listRows(convId)
  })

  const matches = computed<readonly CurrentConversationSearchMatch[]>(() => {
    if (!active.value) return EMPTY_SEARCH_MATCHES
    const found = findCurrentConversationMatches(
      options.messages(),
      options.query.value,
      scopedRows.value
    )
    return found.length > 0 ? found : EMPTY_SEARCH_MATCHES
  })

  const matchMessageIds = computed(() => {
    const found = matches.value
    if (!found.length) return EMPTY_SEARCH_IDS
    const ids = found.filter(match => !match.toolCallId).map(match => match.messageId)
    return ids.length > 0 ? ids : EMPTY_SEARCH_IDS
  })

  const matchToolCallIds = computed(() => {
    const found = matches.value
    if (!found.length) return EMPTY_SEARCH_IDS
    const ids = found.flatMap(match => (match.toolCallId ? [match.toolCallId] : []))
    return ids.length > 0 ? ids : EMPTY_SEARCH_IDS
  })

  const matchContentIds = computed(() => {
    const found = matches.value
    if (!found.length) return EMPTY_SEARCH_IDS
    const ids = found.flatMap(match => (match.contentMessageId ? [match.contentMessageId] : []))
    return ids.length > 0 ? ids : EMPTY_SEARCH_IDS
  })

  return { active, matches, matchMessageIds, matchToolCallIds, matchContentIds }
}
