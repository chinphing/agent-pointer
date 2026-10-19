import { isTauriRuntime } from './runtime'
import {
  backfillTurnElapsed,
  type TurnElapsedBackfillEntry,
  type TurnElapsedBackfillReport
} from './tauri'

/**
 * Legacy blob written by builds before the span moved into
 * `messages.turn_elapsed_ms`.
 *
 * Shape (`TurnElapsedState`, no version field):
 * `{ active: { [conversationId]: { turnId, startedAt } },
 *    completed: { [conversationId + "\u0000" + turnId]: elapsedMs } }`
 *
 * `turnId` is the anchor user message's `message_id`, so `completed` maps
 * directly onto transcript rows. Nothing reads this blob any more; this module
 * folds it into the column once and then deletes the key.
 */
export const LEGACY_TURN_ELAPSED_STORAGE_KEY = 'pointer.chat.turn-elapsed.v1'

export type LegacyBlobParseResult =
  | { ok: true; entries: TurnElapsedBackfillEntry[] }
  | { ok: false; reason: 'invalid-json' | 'invalid-shape' }

export type LegacyTurnElapsedMigrationStatus =
  /** Not the desktop app (web / remote cloud window): nothing to migrate. */
  | 'skipped-not-tauri'
  /** No legacy key on this device. */
  | 'absent'
  /** Blob parsed, but carried no usable finished turn. */
  | 'empty'
  /** Blob was unreadable; it was dropped because no reader remains. */
  | 'corrupt'
  /** Every usable pair was handed to the backend and the key was deleted. */
  | 'migrated'
  /** Backend call failed; the key is kept so the next launch can retry. */
  | 'failed'
  /** Storage itself threw (private mode, disabled storage). */
  | 'error'

export interface LegacyTurnElapsedMigrationResult {
  status: LegacyTurnElapsedMigrationStatus
  /** Usable pairs parsed out of the blob. */
  entries: number
  /** Rows actually written by the backend (0 on replay / failure). */
  updated: number
  /** Backend wall time for the batch. */
  durationMs: number
}

export interface LegacyTurnElapsedMigrationDeps {
  storage: Pick<Storage, 'getItem' | 'removeItem'>
  backfill: (entries: TurnElapsedBackfillEntry[]) => Promise<TurnElapsedBackfillReport>
  /** Defaults to true; the once-wrapper passes the real runtime check. */
  enabled?: boolean
  /** Count-only logging; never pass ids. */
  log?: (message: string, detail?: Record<string, number | string | boolean>) => void
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * Parse the legacy blob into `(conversationId, messageId, elapsedMs)` pairs.
 *
 * Tolerant by design: a single malformed entry is skipped, not fatal. A blob
 * that cannot be understood at all returns `ok: false` — the caller drops it,
 * because nothing reads it and keeping it would only retry forever.
 */
export function parseLegacyTurnElapsedBlob(
  raw: string | null | undefined
): LegacyBlobParseResult {
  if (raw == null || raw.trim() === '') return { ok: true, entries: [] }

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return { ok: false, reason: 'invalid-json' }
  }
  if (!isRecord(parsed) || !isRecord(parsed.completed)) {
    return { ok: false, reason: 'invalid-shape' }
  }

  const entries: TurnElapsedBackfillEntry[] = []
  for (const [key, value] of Object.entries(parsed.completed)) {
    const separator = key.indexOf('\u0000')
    if (separator <= 0) continue
    const conversationId = key.slice(0, separator).trim()
    const messageId = key.slice(separator + 1).trim()
    if (!conversationId || !messageId) continue
    if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) continue
    entries.push({ conversationId, messageId, elapsedMs: Math.round(value) })
  }
  return { ok: true, entries }
}

function removeLegacyKey(
  storage: Pick<Storage, 'removeItem'>,
  log: NonNullable<LegacyTurnElapsedMigrationDeps['log']>
): void {
  try {
    storage.removeItem(LEGACY_TURN_ELAPSED_STORAGE_KEY)
  } catch {
    // Storage disabled / private mode: the blob stays, but it has no reader
    // and the next launch retries the removal. Never block app start.
    log('legacy blob removal failed')
  }
}

/**
 * Failure-safe one-shot migration.
 *
 * Order matters: the backend must accept the batch **before** the key is
 * deleted. A failed command keeps the blob so the next launch can retry; a
 * corrupt or empty blob is deleted anyway (no reader, no usable data).
 * Every failure is swallowed — boot must not depend on this.
 */
export async function runLegacyTurnElapsedMigration(
  deps: LegacyTurnElapsedMigrationDeps
): Promise<LegacyTurnElapsedMigrationResult> {
  const log = deps.log ?? (() => {})
  if (deps.enabled === false) {
    return { status: 'skipped-not-tauri', entries: 0, updated: 0, durationMs: 0 }
  }

  let raw: string | null
  try {
    raw = deps.storage.getItem(LEGACY_TURN_ELAPSED_STORAGE_KEY)
  } catch {
    log('legacy blob read failed')
    return { status: 'error', entries: 0, updated: 0, durationMs: 0 }
  }
  if (raw == null) return { status: 'absent', entries: 0, updated: 0, durationMs: 0 }

  const parsed = parseLegacyTurnElapsedBlob(raw)
  if (!parsed.ok) {
    removeLegacyKey(deps.storage, log)
    log('legacy blob unreadable; dropped', { reason: parsed.reason })
    return { status: 'corrupt', entries: 0, updated: 0, durationMs: 0 }
  }
  if (parsed.entries.length === 0) {
    removeLegacyKey(deps.storage, log)
    log('legacy blob empty; dropped')
    return { status: 'empty', entries: 0, updated: 0, durationMs: 0 }
  }

  let report: TurnElapsedBackfillReport
  try {
    report = await deps.backfill(parsed.entries)
  } catch (error) {
    log('backfill failed; legacy blob kept for the next launch', {
      entries: parsed.entries.length,
      error: error instanceof Error ? error.name : 'unknown'
    })
    return { status: 'failed', entries: parsed.entries.length, updated: 0, durationMs: 0 }
  }

  // The backend accepted the batch (writes + store_meta flag are atomic), so
  // the blob has served its purpose.
  removeLegacyKey(deps.storage, log)
  log('backfill complete', {
    entries: parsed.entries.length,
    updated: report.updated,
    skipped: report.skipped,
    alreadyDone: report.alreadyDone,
    durationMs: report.durationMs
  })
  return {
    status: 'migrated',
    entries: parsed.entries.length,
    updated: report.updated,
    durationMs: report.durationMs
  }
}

let migrationPromise: Promise<LegacyTurnElapsedMigrationResult> | null = null

/**
 * Boot entry point: runs at most once per window and never throws.
 *
 * The backend `store_meta` flag already makes a replay a no-op; this memo just
 * avoids a second IPC round trip if boot code runs twice (HMR, remount).
 */
export function migrateLegacyTurnElapsedOnce(): Promise<LegacyTurnElapsedMigrationResult> {
  if (migrationPromise) return migrationPromise
  const promise = (async (): Promise<LegacyTurnElapsedMigrationResult> => {
    if (!isTauriRuntime() || typeof localStorage === 'undefined') {
      return { status: 'skipped-not-tauri', entries: 0, updated: 0, durationMs: 0 }
    }
    return await runLegacyTurnElapsedMigration({
      storage: localStorage,
      backfill: backfillTurnElapsed,
      log: (message, detail) => console.info('[turn-elapsed migration]', message, detail ?? {})
    })
  })().catch(error => {
    console.warn('[turn-elapsed migration] unexpected failure', error)
    return { status: 'error', entries: 0, updated: 0, durationMs: 0 } as const
  })
  migrationPromise = promise
  return promise
}

/** Test hook: forget the once-per-window memo. */
export function resetLegacyTurnElapsedMigrationForTests(): void {
  migrationPromise = null
}
