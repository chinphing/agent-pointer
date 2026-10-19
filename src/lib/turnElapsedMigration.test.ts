import { describe, expect, it, vi } from 'vitest'
import {
  LEGACY_TURN_ELAPSED_STORAGE_KEY,
  parseLegacyTurnElapsedBlob,
  runLegacyTurnElapsedMigration,
  type LegacyTurnElapsedMigrationDeps
} from './turnElapsedMigration'
import type { TurnElapsedBackfillEntry, TurnElapsedBackfillReport } from './tauri'

/** Minimal in-memory Storage: no real localStorage anywhere in these tests. */
class FakeStorage {
  private map = new Map<string, string>()
  removed: string[] = []
  getItemThrows = false
  removeItemThrows = false

  constructor(seed?: Record<string, string>) {
    for (const [key, value] of Object.entries(seed ?? {})) this.map.set(key, value)
  }

  getItem(key: string): string | null {
    if (this.getItemThrows) throw new Error('storage disabled')
    return this.map.get(key) ?? null
  }

  removeItem(key: string): void {
    if (this.removeItemThrows) throw new Error('storage disabled')
    this.removed.push(key)
    this.map.delete(key)
  }

  has(key: string): boolean {
    return this.map.has(key)
  }
}

function report(overrides: Partial<TurnElapsedBackfillReport> = {}): TurnElapsedBackfillReport {
  return {
    received: 0,
    updated: 0,
    skipped: 0,
    alreadyDone: false,
    durationMs: 1,
    ...overrides
  }
}

function deps(
  storage: FakeStorage,
  backfill: LegacyTurnElapsedMigrationDeps['backfill']
): LegacyTurnElapsedMigrationDeps {
  return { storage, backfill }
}

const conversationId = 'conv-fixture-1'
const otherConversationId = 'conv-fixture-2'
const turnId = 'user-turn-1'
const otherTurnId = 'user-turn-2'
const keyOf = (conversation: string, turn: string) => `${conversation}\u0000${turn}`

function legacyBlob(completed: Record<string, unknown>): string {
  return JSON.stringify({ active: {}, completed })
}

describe('parseLegacyTurnElapsedBlob', () => {
  it('maps completed entries onto (conversationId, messageId, elapsedMs)', () => {
    const parsed = parseLegacyTurnElapsedBlob(
      legacyBlob({
        [keyOf(conversationId, turnId)]: 61_000,
        [keyOf(otherConversationId, otherTurnId)]: 3_500
      })
    )

    expect(parsed).toEqual({
      ok: true,
      entries: [
        { conversationId, messageId: turnId, elapsedMs: 61_000 },
        { conversationId: otherConversationId, messageId: otherTurnId, elapsedMs: 3_500 }
      ]
    })
  })

  it('skips malformed entries instead of failing the whole blob', () => {
    const parsed = parseLegacyTurnElapsedBlob(
      legacyBlob({
        // No separator: not a completed-turn key.
        'no-separator': 1_000,
        // Empty halves.
        [keyOf('', turnId)]: 1_000,
        [keyOf(conversationId, '  ')]: 1_000,
        // Non-finite / negative / non-numeric spans.
        [keyOf(conversationId, 'negative')]: -1,
        [keyOf(conversationId, 'string')]: '1000',
        [keyOf(conversationId, 'null')]: null,
        [keyOf(conversationId, turnId)]: 2_000
      })
    )

    expect(parsed).toEqual({
      ok: true,
      entries: [{ conversationId, messageId: turnId, elapsedMs: 2_000 }]
    })
  })

  it('treats absent / blank storage values as an empty, valid blob', () => {
    expect(parseLegacyTurnElapsedBlob(null)).toEqual({ ok: true, entries: [] })
    expect(parseLegacyTurnElapsedBlob('   ')).toEqual({ ok: true, entries: [] })
  })

  it('reports invalid JSON and invalid shapes instead of throwing', () => {
    expect(parseLegacyTurnElapsedBlob('{not json')).toEqual({
      ok: false,
      reason: 'invalid-json'
    })
    expect(parseLegacyTurnElapsedBlob('[]')).toEqual({ ok: false, reason: 'invalid-shape' })
    expect(parseLegacyTurnElapsedBlob('null')).toEqual({ ok: false, reason: 'invalid-shape' })
    expect(parseLegacyTurnElapsedBlob(JSON.stringify({ active: {} }))).toEqual({
      ok: false,
      reason: 'invalid-shape'
    })
    expect(parseLegacyTurnElapsedBlob(JSON.stringify({ completed: [] }))).toEqual({
      ok: false,
      reason: 'invalid-shape'
    })
  })
})

describe('runLegacyTurnElapsedMigration', () => {
  it('hands parsed pairs to the backend and deletes the key afterwards', async () => {
    const storage = new FakeStorage({
      [LEGACY_TURN_ELAPSED_STORAGE_KEY]: legacyBlob({
        [keyOf(conversationId, turnId)]: 61_000
      })
    })
    const received: TurnElapsedBackfillEntry[][] = []
    const backfill = vi.fn(async (entries: TurnElapsedBackfillEntry[]) => {
      received.push(entries)
      return report({ received: entries.length, updated: entries.length, durationMs: 42 })
    })

    const result = await runLegacyTurnElapsedMigration(deps(storage, backfill))

    expect(backfill).toHaveBeenCalledTimes(1)
    expect(received[0]).toEqual([
      { conversationId, messageId: turnId, elapsedMs: 61_000 }
    ])
    expect(result).toEqual({
      status: 'migrated',
      entries: 1,
      updated: 1,
      durationMs: 42
    })
    expect(storage.has(LEGACY_TURN_ELAPSED_STORAGE_KEY)).toBe(false)
    expect(storage.removed).toEqual([LEGACY_TURN_ELAPSED_STORAGE_KEY])
  })

  it('drops a corrupt blob without calling the backend', async () => {
    for (const raw of ['{not json', JSON.stringify({ active: {} }), 'null']) {
      const storage = new FakeStorage({ [LEGACY_TURN_ELAPSED_STORAGE_KEY]: raw })
      const backfill = vi.fn(async () => report())

      const result = await runLegacyTurnElapsedMigration(deps(storage, backfill))

      expect(result.status).toBe('corrupt')
      expect(backfill).not.toHaveBeenCalled()
      expect(storage.has(LEGACY_TURN_ELAPSED_STORAGE_KEY)).toBe(false)
    }
  })

  it('drops an empty completed map without calling the backend', async () => {
    const storage = new FakeStorage({
      [LEGACY_TURN_ELAPSED_STORAGE_KEY]: legacyBlob({})
    })
    const backfill = vi.fn(async () => report())

    const result = await runLegacyTurnElapsedMigration(deps(storage, backfill))

    expect(result.status).toBe('empty')
    expect(backfill).not.toHaveBeenCalled()
    expect(storage.has(LEGACY_TURN_ELAPSED_STORAGE_KEY)).toBe(false)
  })

  it('keeps the blob when the backend fails, so the next launch can retry', async () => {
    const storage = new FakeStorage({
      [LEGACY_TURN_ELAPSED_STORAGE_KEY]: legacyBlob({
        [keyOf(conversationId, turnId)]: 61_000
      })
    })
    const backfill = vi.fn(async () => {
      throw new Error('ipc down')
    })

    const result = await runLegacyTurnElapsedMigration(deps(storage, backfill))

    expect(result).toMatchObject({ status: 'failed', entries: 1, updated: 0 })
    expect(storage.has(LEGACY_TURN_ELAPSED_STORAGE_KEY)).toBe(true)
    expect(storage.removed).toEqual([])
  })

  it('does nothing when the key is absent or the runtime is not desktop', async () => {
    const absent = new FakeStorage()
    const backfill = vi.fn(async () => report())
    expect(await runLegacyTurnElapsedMigration(deps(absent, backfill))).toMatchObject({
      status: 'absent'
    })
    expect(backfill).not.toHaveBeenCalled()

    const seeded = new FakeStorage({
      [LEGACY_TURN_ELAPSED_STORAGE_KEY]: legacyBlob({
        [keyOf(conversationId, turnId)]: 61_000
      })
    })
    const getItem = vi.spyOn(seeded, 'getItem')
    expect(
      await runLegacyTurnElapsedMigration({
        ...deps(seeded, backfill),
        enabled: false
      })
    ).toMatchObject({ status: 'skipped-not-tauri' })
    expect(getItem).not.toHaveBeenCalled()
    expect(seeded.has(LEGACY_TURN_ELAPSED_STORAGE_KEY)).toBe(true)
  })

  it('survives a storage read or removal that throws', async () => {
    const unreadable = new FakeStorage()
    unreadable.getItemThrows = true
    const backfill = vi.fn(async () => report())
    expect(await runLegacyTurnElapsedMigration(deps(unreadable, backfill))).toMatchObject({
      status: 'error'
    })
    expect(backfill).not.toHaveBeenCalled()

    const unremovable = new FakeStorage({
      [LEGACY_TURN_ELAPSED_STORAGE_KEY]: legacyBlob({
        [keyOf(conversationId, turnId)]: 61_000
      })
    })
    unremovable.removeItemThrows = true
    const result = await runLegacyTurnElapsedMigration(deps(unremovable, backfill))
    expect(result.status).toBe('migrated')
    expect(unremovable.has(LEGACY_TURN_ELAPSED_STORAGE_KEY)).toBe(true)
  })

  it('reports a backend replay as migrated and still drops the key', async () => {
    const storage = new FakeStorage({
      [LEGACY_TURN_ELAPSED_STORAGE_KEY]: legacyBlob({
        [keyOf(conversationId, turnId)]: 61_000
      })
    })
    const backfill = vi.fn(async (entries: TurnElapsedBackfillEntry[]) =>
      report({ received: entries.length, alreadyDone: true, durationMs: 3 })
    )

    const result = await runLegacyTurnElapsedMigration(deps(storage, backfill))

    expect(result).toMatchObject({ status: 'migrated', entries: 1, updated: 0 })
    expect(storage.has(LEGACY_TURN_ELAPSED_STORAGE_KEY)).toBe(false)
  })
})
