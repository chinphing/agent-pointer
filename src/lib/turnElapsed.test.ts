// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  activeTurnStartedAt,
  elapsedBetweenTimestamps,
  formatTurnElapsed,
  hasActiveTurn,
  peekActiveTurn,
  recordTurnDone,
  recordTurnDoneWithSpan,
  recordTurnStart,
  resolveTurnElapsedMs,
  turnElapsedMs,
  turnMessageCreatedAtSpan
} from './turnElapsed'
import type { ChatMessage } from '../types/chat'
import { applyUiLocale } from './uiLocale'
import { createJsonParseStats, installJsonParseProbe } from './jsonParseProbe'

function msg(
  id: string,
  role: ChatMessage['role'],
  createdAt: number,
  extra: Partial<ChatMessage> = {}
): ChatMessage {
  return { id, role, content: '', status: 'done', createdAt, ...extra }
}

describe('turn elapsed', () => {
  beforeEach(() => {
    localStorage.clear()
    // `formatTurnElapsed` renders through the shared i18n instance, whose locale
    // comes from localStorage / `navigator.language` (ambient, English in
    // happy-dom). Pin it so the localized expectations below are deterministic.
    applyUiLocale('zh-CN')
  })

  it('records dispatch-to-done elapsed time by conversation and user turn', () => {
    recordTurnStart('conv-1', 'user-1', 1_000)
    expect(hasActiveTurn('conv-1')).toBe(true)

    expect(recordTurnDone('conv-1', 66_400)).toBe(65_400)
    expect(hasActiveTurn('conv-1')).toBe(false)
    expect(turnElapsedMs('conv-1', 'user-1')).toBe(65_400)
    expect(turnElapsedMs('conv-1', 'other-user')).toBeNull()
  })

  it('overrides elapsed with backend run_chat timestamps for the active turn', () => {
    recordTurnStart('conv-1', 'user-1', 1_000)

    // Backend span (20s→50s) wins over local dispatch timing (1s→now).
    expect(recordTurnDoneWithSpan('conv-1', 'user-1', 20_000, 50_000)).toBe(30_000)
    expect(hasActiveTurn('conv-1')).toBe(false)
    expect(turnElapsedMs('conv-1', 'user-1')).toBe(30_000)
  })

  it('ignores backend span when the turn id does not match the active turn', () => {
    recordTurnStart('conv-1', 'user-1', 1_000)

    expect(recordTurnDoneWithSpan('conv-1', 'other-turn', 20_000, 50_000)).toBeNull()
    expect(hasActiveTurn('conv-1')).toBe(true)
    expect(turnElapsedMs('conv-1', 'user-1')).toBeNull()
  })

  it('ignores invalid backend spans without clearing the active turn', () => {
    recordTurnStart('conv-1', 'user-1', 1_000)

    expect(recordTurnDoneWithSpan('conv-1', 'user-1', 50_000, 20_000)).toBeNull()
    expect(recordTurnDoneWithSpan('conv-1', 'user-1', Number.NaN, 50_000)).toBeNull()
    expect(hasActiveTurn('conv-1')).toBe(true)
  })

  it('survives a storage read and keeps conversations isolated', () => {
    recordTurnStart('conv-1', 'user-1', 5_000)
    recordTurnStart('conv-2', 'user-2', 10_000)
    recordTurnDone('conv-1', 7_500)

    expect(turnElapsedMs('conv-1', 'user-1')).toBe(2_500)
    expect(turnElapsedMs('conv-2', 'user-2')).toBeNull()
    expect(recordTurnDone('conv-2', 14_000)).toBe(4_000)
  })

  it('finalizes the interrupted turn when force-send starts the next turn', () => {
    recordTurnStart('conv-1', 'user-a', 1_000)
    recordTurnStart('conv-1', 'user-b', 4_000)

    expect(turnElapsedMs('conv-1', 'user-a')).toBe(3_000)
    expect(hasActiveTurn('conv-1')).toBe(true)
    expect(recordTurnDone('conv-1', 9_000)).toBe(5_000)
    expect(turnElapsedMs('conv-1', 'user-b')).toBe(5_000)
  })

  it('reports the running turn start for live ticking, null otherwise', () => {
    expect(activeTurnStartedAt('conv-1', 'user-1')).toBeNull()

    recordTurnStart('conv-1', 'user-1', 12_345)
    expect(activeTurnStartedAt('conv-1', 'user-1')).toBe(12_345)
    // Another conversation / different turn id does not match the active one.
    expect(activeTurnStartedAt('conv-2', 'user-1')).toBeNull()
    expect(activeTurnStartedAt('conv-1', 'user-2')).toBeNull()

    recordTurnDone('conv-1', 20_000)
    expect(activeTurnStartedAt('conv-1', 'user-1')).toBeNull()
  })

  it('derives elapsed time from persisted message timestamps', () => {
    expect(elapsedBetweenTimestamps(10_000, 75_400)).toBe(65_400)
    expect(elapsedBetweenTimestamps(75_400, 10_000)).toBeNull()
    expect(elapsedBetweenTimestamps(Number.NaN, 10_000)).toBeNull()
  })

  it('prefers recorded dispatch→Done timing over message createdAt span', () => {
    recordTurnStart('conv-1', 'user-1', 1_000)
    recordTurnDone('conv-1', 61_000)

    // createdAt span includes queue wait (enqueue at 0, last msg at 70s) — must not win.
    expect(
      resolveTurnElapsedMs({
        conversationId: 'conv-1',
        turnId: 'user-1',
        userCreatedAt: 0,
        lastMessageCreatedAt: 70_000
      })
    ).toBe(60_000)
  })

  it('falls back to createdAt span when no recorded timing exists', () => {
    expect(
      resolveTurnElapsedMs({
        conversationId: 'conv-1',
        turnId: 'user-missing',
        userCreatedAt: 10_000,
        lastMessageCreatedAt: 25_000
      })
    ).toBe(15_000)
  })

  it('ignores a sub-second recorded span when message timestamps show longer work', () => {
    recordTurnStart('conv-1', 'user-1', 50_000)
    recordTurnDone('conv-1', 50_200)

    expect(
      resolveTurnElapsedMs({
        conversationId: 'conv-1',
        turnId: 'user-1',
        userCreatedAt: 10_000,
        lastMessageCreatedAt: 70_000
      })
    ).toBe(60_000)
  })

  it('keeps a genuine sub-second recorded span when createdAt is not longer', () => {
    recordTurnStart('conv-1', 'user-fast', 1_000)
    recordTurnDone('conv-1', 1_400)

    expect(
      resolveTurnElapsedMs({
        conversationId: 'conv-1',
        turnId: 'user-fast',
        userCreatedAt: 1_000,
        lastMessageCreatedAt: 1_300
      })
    ).toBe(400)
  })

  it('does not end the createdAt window on scoped sub-agent user stubs', () => {
    const messages: ChatMessage[] = [
      msg('user-1', 'user', 1_000, { content: 'go' }),
      msg('asst-1', 'assistant', 1_500),
      msg('sub_task_1', 'user', 1_600, {
        content: 'Begin. Your assigned task is in the system prompt',
        anchorMessageId: 'asst-1'
      }),
      msg('asst-final', 'assistant', 90_000, { content: 'done' }),
      msg('user-2', 'user', 100_000, { content: 'next' })
    ]
    expect(turnMessageCreatedAtSpan(messages, 'user-1')).toEqual({
      userCreatedAt: 1_000,
      lastMessageCreatedAt: 90_000
    })
  })

  it('formats Cursor-style minutes and zero-padded seconds with an honest fallback', () => {
    expect(formatTurnElapsed(125_999)).toBe('工作 2 m 05 s')
    expect(formatTurnElapsed(900)).toBe('工作 0 m 00 s')
    expect(formatTurnElapsed(null)).toBe('工作耗时未知')
    expect(formatTurnElapsed(null, 'active')).toBe('工作耗时未知')
  })
})

/**
 * The blob is one entry per finished turn with no pruning, so a long-lived
 * install reaches ~877k characters (measured: 1,754,628 bytes in WebKit's
 * UTF-16 storage = 877,314 chars, the exact figure the render HUD reported).
 * `MessageList` asks for a turn's elapsed time per visible turn per frame, so
 * every render used to re-parse all of it. These tests pin both halves of the
 * contract: the parse happens once per *change*, and the answers do not move.
 */
describe('turn elapsed storage reads', () => {
  const STORAGE_KEY = 'pointer.chat.turn-elapsed.v1'

  /**
   * Minimal `localStorage`. happy-dom's own implementation parses its storage
   * blob lazily, which would show up in the probe counts below; this keeps the
   * accounting about `turnElapsed` alone.
   */
  class MemoryStorage {
    private map = new Map<string, string>()
    get length(): number {
      return this.map.size
    }
    key(index: number): string | null {
      return [...this.map.keys()][index] ?? null
    }
    getItem(key: string): string | null {
      return this.map.get(key) ?? null
    }
    setItem(key: string, value: string): void {
      this.map.set(key, String(value))
    }
    removeItem(key: string): void {
      this.map.delete(key)
    }
    clear(): void {
      this.map.clear()
    }
  }

  /** The shape/size of the real blob: many finished turns, one active turn. */
  function bigStoredBlob(entries = 40_000): string {
    const completed: Record<string, number> = {}
    for (let i = 0; i < entries; i += 1) {
      completed[`conv-${i % 50}\u0000turn-${i}`] = 60_000 + i
    }
    return JSON.stringify({
      active: { 'conv-0': { turnId: 'turn-live', startedAt: 1_000 } },
      completed
    })
  }

  beforeEach(() => {
    vi.stubGlobal('localStorage', new MemoryStorage())
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('parses an unchanged blob once instead of once per render read', () => {
    const raw = bigStoredBlob()
    expect(raw.length).toBeGreaterThan(800_000)
    localStorage.setItem(STORAGE_KEY, raw)

    const stats = createJsonParseStats()
    const dispose = installJsonParseProbe({ stats })
    try {
      const beforeFirst = { calls: stats.calls, chars: stats.totalChars }
      expect(activeTurnStartedAt('conv-0', 'turn-live')).toBe(1_000)
      const afterFirst = { calls: stats.calls, chars: stats.totalChars }
      // The blob was parsed, and it is the largest single parse by far.
      expect(stats.maxChars).toBe(raw.length)
      expect(afterFirst.chars - beforeFirst.chars).toBeLessThan(raw.length * 1.1)

      // One second of streaming: `turnElapsedLabel()` (chip + label) per turn
      // per frame. 400 reads used to mean 400 parses of the whole blob.
      for (let i = 0; i < 400; i += 1) {
        activeTurnStartedAt('conv-0', 'turn-live')
        resolveTurnElapsedMs({
          conversationId: 'conv-0',
          turnId: 'turn-live',
          userCreatedAt: 1_000,
          lastMessageCreatedAt: 2_000
        })
        turnElapsedMs('conv-1', 'turn-1')
        hasActiveTurn('conv-0')
        peekActiveTurn('conv-0')
      }

      expect(stats.calls - afterFirst.calls).toBe(0)
      expect(stats.totalChars - afterFirst.chars).toBe(0)
    } finally {
      dispose()
    }
  })

  it('answers exactly what a fresh read of the blob answers', () => {
    const raw = JSON.stringify({
      active: { 'conv-a': { turnId: 'user-9', startedAt: 4_000 } },
      completed: { 'conv-a\u0000user-1': 65_400, 'conv-b\u0000user-2': 400 }
    })
    localStorage.setItem(STORAGE_KEY, raw)
    const reference = JSON.parse(raw) as {
      active: Record<string, { turnId: string; startedAt: number }>
      completed: Record<string, number>
    }

    expect(peekActiveTurn('conv-a')).toEqual(reference.active['conv-a'])
    expect(peekActiveTurn('conv-b')).toBeNull()
    expect(hasActiveTurn('conv-a')).toBe(true)
    expect(activeTurnStartedAt('conv-a', 'user-9')).toBe(4_000)
    expect(activeTurnStartedAt('conv-a', 'user-1')).toBeNull()
    expect(turnElapsedMs('conv-a', 'user-1')).toBe(reference.completed['conv-a\u0000user-1'])
    expect(turnElapsedMs('conv-b', 'user-2')).toBe(reference.completed['conv-b\u0000user-2'])
    expect(
      resolveTurnElapsedMs({
        conversationId: 'conv-a',
        turnId: 'user-1',
        userCreatedAt: 1_000,
        lastMessageCreatedAt: 999_999
      })
    ).toBe(65_400)
  })

  it('persists the same bytes the read-every-time implementation wrote', () => {
    const seed = JSON.stringify({
      active: { 'conv-1': { turnId: 'user-0', startedAt: 100 } },
      completed: { 'conv-0\u0000user-0': 5_000 }
    })
    localStorage.setItem(STORAGE_KEY, seed)

    recordTurnStart('conv-1', 'user-1', 1_000)
    recordTurnDone('conv-1', 66_400)

    // Reference: the previous implementation's fold, applied to the same seed.
    const state = JSON.parse(seed) as {
      active: Record<string, { turnId: string; startedAt: number }>
      completed: Record<string, number>
    }
    const prev = state.active['conv-1']!
    state.completed['conv-1\u0000user-0'] = 1_000 - prev.startedAt
    state.active['conv-1'] = { turnId: 'user-1', startedAt: 1_000 }
    state.completed['conv-1\u0000user-1'] = 66_400 - 1_000
    delete state.active['conv-1']

    expect(localStorage.getItem(STORAGE_KEY)).toBe(JSON.stringify(state))
  })

  it('re-reads after another window rewrites the blob', () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ active: {}, completed: { 'conv-1\u0000user-1': 1_000 } })
    )
    expect(turnElapsedMs('conv-1', 'user-1')).toBe(1_000)

    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ active: {}, completed: { 'conv-1\u0000user-1': 2_000 } })
    )
    expect(turnElapsedMs('conv-1', 'user-1')).toBe(2_000)
  })

  it('does not re-parse a corrupt blob on every read', () => {
    // A valid blob first: memory holds data that a corrupt read must not revive.
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ active: {}, completed: { 'conv-1\u0000user-1': 1_000 } })
    )
    expect(turnElapsedMs('conv-1', 'user-1')).toBe(1_000)

    localStorage.setItem(STORAGE_KEY, '{not json')
    const stats = createJsonParseStats()
    const dispose = installJsonParseProbe({ stats })
    try {
      expect(turnElapsedMs('conv-1', 'user-1')).toBeNull()
      const calls = stats.calls
      for (let i = 0; i < 50; i += 1) expect(turnElapsedMs('conv-1', 'user-1')).toBeNull()
      expect(stats.calls - calls).toBe(0)
    } finally {
      dispose()
    }

    // Corrupt storage must not block recording: the next write repairs the blob.
    recordTurnStart('conv-1', 'user-2', 5_000)
    expect(peekActiveTurn('conv-1')).toEqual({ turnId: 'user-2', startedAt: 5_000 })
  })
})
