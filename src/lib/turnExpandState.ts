/**
 * Per-conversation turn expand / collapse UI overrides, scoped per platform user.
 *
 * MessageList used to wipe expand sets on `currentId` change, so manually
 * collapsed turns re-opened via `shouldAutoExpandTurn` after switching away
 * and back. Persist to localStorage (same pattern as turn-elapsed) so refresh
 * keeps the user's intent; memory Map is a write-through cache.
 *
 * Storage is keyed by `userId` from the platform session so accounts on the
 * same browser/device do not share overrides. Logout clears memory only;
 * each user's disk bucket is kept for the next login.
 *
 * Only explicit overrides are stored. Implicit auto-expand
 * (`shouldAutoExpandTurn` / change-summary heuristics) is recomputed.
 */

export type TurnExpandUiState = {
  expandedTurnIds: ReadonlySet<string>
  manuallyCollapsedTurnIds: ReadonlySet<string>
  expandedChangeTurnIds: ReadonlySet<string>
  collapsedChangeTurnIds: ReadonlySet<string>
}

type MutableTurnExpandUiState = {
  expandedTurnIds: Set<string>
  manuallyCollapsedTurnIds: Set<string>
  expandedChangeTurnIds: Set<string>
  collapsedChangeTurnIds: Set<string>
}

type StoredConversation = {
  expandedTurnIds: string[]
  manuallyCollapsedTurnIds: string[]
  expandedChangeTurnIds: string[]
  collapsedChangeTurnIds: string[]
  /** Last save time; used for LRU eviction. */
  updatedAt: number
}

type StoredBlob = {
  v: 1
  byConversation: Record<string, StoredConversation>
}

/** Unscoped legacy key (pre user-isolation); migrated once into the active scope. */
export const TURN_EXPAND_LEGACY_STORAGE_KEY = 'pointer.chat.turn-expand.v1'

const STORAGE_KEY_PREFIX = 'pointer.chat.turn-expand.v1'

/** Soft cap so long-lived browsers do not grow unbounded. */
const MAX_CONVERSATIONS = 80

const ANON_SCOPE = 'anon'

const byConversation = new Map<string, MutableTurnExpandUiState>()
let activeScope = ANON_SCOPE

/**
 * Conversations whose expand/collapse state is retained. Read-only: eviction
 * (`MAX_CONVERSATIONS`) and the scope reset stay the only writers — this is the
 * row that shows whether the map is holding far more conversations than the cap
 * (see `lib/residencyProbe.ts`).
 */
export function turnExpandConversationCount(): number {
  return byConversation.size
}

function cloneSet(ids: ReadonlySet<string>): Set<string> {
  return new Set(ids)
}

export function emptyTurnExpandUiState(): MutableTurnExpandUiState {
  return {
    expandedTurnIds: new Set(),
    manuallyCollapsedTurnIds: new Set(),
    expandedChangeTurnIds: new Set(),
    collapsedChangeTurnIds: new Set()
  }
}

/** Prefer collapse when both sets claim the same id (corrupt / race). */
export function sanitizeTurnExpandUiState(state: TurnExpandUiState): MutableTurnExpandUiState {
  const expandedTurnIds = cloneSet(state.expandedTurnIds)
  const manuallyCollapsedTurnIds = cloneSet(state.manuallyCollapsedTurnIds)
  for (const id of manuallyCollapsedTurnIds) expandedTurnIds.delete(id)

  const expandedChangeTurnIds = cloneSet(state.expandedChangeTurnIds)
  const collapsedChangeTurnIds = cloneSet(state.collapsedChangeTurnIds)
  for (const id of collapsedChangeTurnIds) expandedChangeTurnIds.delete(id)

  return {
    expandedTurnIds,
    manuallyCollapsedTurnIds,
    expandedChangeTurnIds,
    collapsedChangeTurnIds
  }
}

export function cloneTurnExpandUiState(state: TurnExpandUiState): MutableTurnExpandUiState {
  return sanitizeTurnExpandUiState(state)
}

export function isEmptyTurnExpandUiState(state: TurnExpandUiState): boolean {
  return (
    state.expandedTurnIds.size === 0
    && state.manuallyCollapsedTurnIds.size === 0
    && state.expandedChangeTurnIds.size === 0
    && state.collapsedChangeTurnIds.size === 0
  )
}

/** Normalize platform user id into a localStorage-safe scope segment. */
export function normalizeTurnExpandStorageScope(userId: string | null | undefined): string {
  const id = userId?.trim()
  if (!id) return ANON_SCOPE
  return id.replace(/[^\w.@+-]/g, '_').slice(0, 200) || ANON_SCOPE
}

export function turnExpandStorageKey(scope: string = activeScope): string {
  return `${STORAGE_KEY_PREFIX}::${scope}`
}

export function getTurnExpandStorageScope(): string {
  return activeScope
}

/**
 * Bind expand prefs to the logged-in platform user (or anon when logged out).
 * Swapping scope clears the in-memory cache so another account cannot see
 * the previous user's overrides in this tab.
 */
export function setTurnExpandStorageScope(userId: string | null | undefined): void {
  const next = normalizeTurnExpandStorageScope(userId)
  if (next === activeScope) {
    migrateLegacyBlobIfNeeded(next)
    return
  }
  byConversation.clear()
  activeScope = next
  migrateLegacyBlobIfNeeded(next)
}

function toStored(state: TurnExpandUiState, updatedAt: number): StoredConversation {
  const clean = sanitizeTurnExpandUiState(state)
  return {
    expandedTurnIds: [...clean.expandedTurnIds],
    manuallyCollapsedTurnIds: [...clean.manuallyCollapsedTurnIds],
    expandedChangeTurnIds: [...clean.expandedChangeTurnIds],
    collapsedChangeTurnIds: [...clean.collapsedChangeTurnIds],
    updatedAt
  }
}

function fromStored(raw: StoredConversation | undefined): MutableTurnExpandUiState {
  if (!raw || typeof raw !== 'object') return emptyTurnExpandUiState()
  const asSet = (value: unknown): Set<string> => {
    if (!Array.isArray(value)) return new Set()
    return new Set(value.filter((id): id is string => typeof id === 'string' && id.trim() !== ''))
  }
  return sanitizeTurnExpandUiState({
    expandedTurnIds: asSet(raw.expandedTurnIds),
    manuallyCollapsedTurnIds: asSet(raw.manuallyCollapsedTurnIds),
    expandedChangeTurnIds: asSet(raw.expandedChangeTurnIds),
    collapsedChangeTurnIds: asSet(raw.collapsedChangeTurnIds)
  })
}

function readBlobForKey(storageKey: string): StoredBlob {
  if (typeof localStorage === 'undefined') return { v: 1, byConversation: {} }
  try {
    const parsed = JSON.parse(localStorage.getItem(storageKey) ?? '') as Partial<StoredBlob>
    if (parsed?.v !== 1 || !parsed.byConversation || typeof parsed.byConversation !== 'object') {
      return { v: 1, byConversation: {} }
    }
    return { v: 1, byConversation: parsed.byConversation }
  } catch {
    return { v: 1, byConversation: {} }
  }
}

function readBlob(): StoredBlob {
  return readBlobForKey(turnExpandStorageKey())
}

function writeBlobForKey(storageKey: string, blob: StoredBlob): void {
  if (typeof localStorage === 'undefined') return
  try {
    if (Object.keys(blob.byConversation).length === 0) {
      localStorage.removeItem(storageKey)
      return
    }
    localStorage.setItem(storageKey, JSON.stringify(blob))
  } catch (err) {
    console.warn('[turnExpandState] localStorage write failed', err)
  }
}

function writeBlob(blob: StoredBlob): void {
  writeBlobForKey(turnExpandStorageKey(), blob)
}

/** One-shot: move pre-isolation blob into the active user scope. */
function migrateLegacyBlobIfNeeded(scope: string): void {
  if (typeof localStorage === 'undefined') return
  let legacyRaw: string | null
  try {
    legacyRaw = localStorage.getItem(TURN_EXPAND_LEGACY_STORAGE_KEY)
  } catch {
    return
  }
  if (!legacyRaw) return

  const scopedKey = turnExpandStorageKey(scope)
  try {
    const existing = localStorage.getItem(scopedKey)
    if (!existing) {
      const legacy = readBlobForKey(TURN_EXPAND_LEGACY_STORAGE_KEY)
      if (Object.keys(legacy.byConversation).length > 0) {
        writeBlobForKey(scopedKey, legacy)
        console.info('[turnExpandState] migrated legacy expand prefs into scope', scope)
      }
    }
    localStorage.removeItem(TURN_EXPAND_LEGACY_STORAGE_KEY)
  } catch (err) {
    console.warn('[turnExpandState] legacy migrate failed', err)
  }
}

function evictLru(blob: StoredBlob, keepId?: string): void {
  if (Object.keys(blob.byConversation).length <= MAX_CONVERSATIONS) return
  const ranked = Object.keys(blob.byConversation)
    .map(id => ({ id, at: blob.byConversation[id]?.updatedAt ?? 0 }))
    .sort((a, b) => a.at - b.at)
  for (const { id } of ranked) {
    if (Object.keys(blob.byConversation).length <= MAX_CONVERSATIONS) break
    if (keepId && id === keepId) continue
    delete blob.byConversation[id]
    byConversation.delete(id)
  }
}

function persistConversation(id: string, state: TurnExpandUiState): void {
  const clean = sanitizeTurnExpandUiState(state)
  byConversation.set(id, clean)

  const blob = readBlob()
  if (isEmptyTurnExpandUiState(clean)) {
    delete blob.byConversation[id]
  } else {
    blob.byConversation[id] = toStored(clean, Date.now())
    evictLru(blob, id)
  }
  writeBlob(blob)
}

export function saveTurnExpandUiState(
  conversationId: string | null | undefined,
  state: TurnExpandUiState
): void {
  const id = conversationId?.trim()
  if (!id) return
  persistConversation(id, state)
}

export function loadTurnExpandUiState(
  conversationId: string | null | undefined
): MutableTurnExpandUiState {
  const id = conversationId?.trim()
  if (!id) return emptyTurnExpandUiState()

  const cached = byConversation.get(id)
  if (cached) return cloneTurnExpandUiState(cached)

  const fromDisk = fromStored(readBlob().byConversation[id])
  // Hydrate memory so later saves do not treat this as a cold empty session.
  byConversation.set(id, cloneTurnExpandUiState(fromDisk))
  return cloneTurnExpandUiState(fromDisk)
}

/** Drop cached UI when a conversation is deleted from the sidebar. */
export function clearTurnExpandUiState(conversationId: string | null | undefined): void {
  const id = conversationId?.trim()
  if (!id) return
  byConversation.delete(id)
  const blob = readBlob()
  if (!(id in blob.byConversation)) return
  delete blob.byConversation[id]
  writeBlob(blob)
}

/**
 * Drop in-memory overrides (logout / tests). Does not erase other users'
 * localStorage buckets — call after switching scope or before setting anon.
 */
export function clearAllTurnExpandUiState(): void {
  byConversation.clear()
}

/** Test helper: wipe every expand storage key (legacy + scoped) and reset scope. */
export function resetTurnExpandUiStateForTests(): void {
  byConversation.clear()
  activeScope = ANON_SCOPE
  if (typeof localStorage === 'undefined') return
  try {
    const keys: string[] = []
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i)
      if (!key) continue
      if (key === TURN_EXPAND_LEGACY_STORAGE_KEY || key.startsWith(`${STORAGE_KEY_PREFIX}::`)) {
        keys.push(key)
      }
    }
    for (const key of keys) localStorage.removeItem(key)
  } catch (err) {
    console.warn('[turnExpandState] test reset failed', err)
  }
}
