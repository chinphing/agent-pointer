// @vitest-environment happy-dom

import { beforeEach, describe, expect, it } from 'vitest'
import {
  activeTurnStartedAt,
  elapsedBetweenTimestamps,
  formatTurnElapsed,
  hasActiveTurn,
  recordTurnDone,
  recordTurnDoneWithSpan,
  recordTurnStart,
  resolveTurnElapsedMs,
  turnElapsedMs,
  turnMessageCreatedAtSpan
} from './turnElapsed'
import type { ChatMessage } from '../types/chat'
import { applyUiLocale } from './uiLocale'

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
