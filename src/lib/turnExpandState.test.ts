// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  TURN_EXPAND_LEGACY_STORAGE_KEY,
  clearAllTurnExpandUiState,
  clearTurnExpandUiState,
  emptyTurnExpandUiState,
  getTurnExpandStorageScope,
  isEmptyTurnExpandUiState,
  loadTurnExpandUiState,
  normalizeTurnExpandStorageScope,
  resetTurnExpandUiStateForTests,
  sanitizeTurnExpandUiState,
  saveTurnExpandUiState,
  setTurnExpandStorageScope,
  turnExpandStorageKey
} from './turnExpandState'

beforeEach(() => {
  resetTurnExpandUiStateForTests()
})

afterEach(() => {
  resetTurnExpandUiStateForTests()
})

describe('turnExpandState', () => {
  it('returns empty state for unknown or blank conversation ids', () => {
    expect(loadTurnExpandUiState(null).expandedTurnIds.size).toBe(0)
    expect(loadTurnExpandUiState('  ').manuallyCollapsedTurnIds.size).toBe(0)
    expect(loadTurnExpandUiState('missing').expandedChangeTurnIds.size).toBe(0)
  })

  it('restores manually collapsed turns after save', () => {
    setTurnExpandStorageScope('user-a')
    const state = emptyTurnExpandUiState()
    state.manuallyCollapsedTurnIds.add('turn-1')
    state.expandedTurnIds.add('turn-2')
    saveTurnExpandUiState('conv-a', state)

    const loaded = loadTurnExpandUiState('conv-a')
    expect(loaded.manuallyCollapsedTurnIds.has('turn-1')).toBe(true)
    expect(loaded.expandedTurnIds.has('turn-2')).toBe(true)
    expect(loaded.expandedTurnIds.has('turn-1')).toBe(false)
  })

  it('isolates state per conversation and clones on load', () => {
    setTurnExpandStorageScope('user-a')
    const a = emptyTurnExpandUiState()
    a.manuallyCollapsedTurnIds.add('u1')
    saveTurnExpandUiState('conv-a', a)

    const b = emptyTurnExpandUiState()
    b.expandedTurnIds.add('u2')
    saveTurnExpandUiState('conv-b', b)

    const loadedA = loadTurnExpandUiState('conv-a')
    loadedA.manuallyCollapsedTurnIds.add('mutated')
    expect(loadTurnExpandUiState('conv-a').manuallyCollapsedTurnIds.has('mutated')).toBe(false)
    expect(loadTurnExpandUiState('conv-b').expandedTurnIds.has('u2')).toBe(true)
    expect(loadTurnExpandUiState('conv-b').manuallyCollapsedTurnIds.has('u1')).toBe(false)
  })

  it('clears one conversation without touching others', () => {
    setTurnExpandStorageScope('user-a')
    const state = emptyTurnExpandUiState()
    state.collapsedChangeTurnIds.add('t1')
    saveTurnExpandUiState('conv-a', state)
    saveTurnExpandUiState('conv-b', state)
    clearTurnExpandUiState('conv-a')
    expect(loadTurnExpandUiState('conv-a').collapsedChangeTurnIds.size).toBe(0)
    expect(loadTurnExpandUiState('conv-b').collapsedChangeTurnIds.has('t1')).toBe(true)
  })

  it('survives a cold load from localStorage (simulates refresh)', () => {
    setTurnExpandStorageScope('user-a')
    const state = emptyTurnExpandUiState()
    state.manuallyCollapsedTurnIds.add('turn-x')
    saveTurnExpandUiState('conv-a', state)

    const key = turnExpandStorageKey('user-a')
    const raw = localStorage.getItem(key)
    expect(raw).toBeTruthy()

    clearAllTurnExpandUiState()
    expect(loadTurnExpandUiState('conv-a').manuallyCollapsedTurnIds.has('turn-x')).toBe(true)
  })

  it('does not wipe disk when saving empty after a fresh memory miss hydrates first', () => {
    setTurnExpandStorageScope('user-a')
    localStorage.setItem(
      turnExpandStorageKey('user-a'),
      JSON.stringify({
        v: 1,
        byConversation: {
          'conv-a': {
            expandedTurnIds: [],
            manuallyCollapsedTurnIds: ['kept'],
            expandedChangeTurnIds: [],
            collapsedChangeTurnIds: [],
            updatedAt: 1
          }
        }
      })
    )
    const loaded = loadTurnExpandUiState('conv-a')
    expect(loaded.manuallyCollapsedTurnIds.has('kept')).toBe(true)
    saveTurnExpandUiState('conv-a', loaded)
    expect(loadTurnExpandUiState('conv-a').manuallyCollapsedTurnIds.has('kept')).toBe(true)
  })

  it('removes the conversation entry when overrides become empty', () => {
    setTurnExpandStorageScope('user-a')
    const state = emptyTurnExpandUiState()
    state.expandedTurnIds.add('t1')
    saveTurnExpandUiState('conv-a', state)
    expect(localStorage.getItem(turnExpandStorageKey('user-a'))).toBeTruthy()

    saveTurnExpandUiState('conv-a', emptyTurnExpandUiState())
    expect(isEmptyTurnExpandUiState(loadTurnExpandUiState('conv-a'))).toBe(true)
    expect(localStorage.getItem(turnExpandStorageKey('user-a'))).toBeNull()
  })

  it('sanitize prefers collapse when expand and collapse both list an id', () => {
    const dirty = emptyTurnExpandUiState()
    dirty.expandedTurnIds.add('t1')
    dirty.manuallyCollapsedTurnIds.add('t1')
    dirty.expandedChangeTurnIds.add('c1')
    dirty.collapsedChangeTurnIds.add('c1')
    const clean = sanitizeTurnExpandUiState(dirty)
    expect(clean.expandedTurnIds.has('t1')).toBe(false)
    expect(clean.manuallyCollapsedTurnIds.has('t1')).toBe(true)
    expect(clean.expandedChangeTurnIds.has('c1')).toBe(false)
    expect(clean.collapsedChangeTurnIds.has('c1')).toBe(true)
  })

  it('isolates expand prefs across users and keeps other users on logout clear', () => {
    setTurnExpandStorageScope('alice')
    const alice = emptyTurnExpandUiState()
    alice.manuallyCollapsedTurnIds.add('turn-alice')
    saveTurnExpandUiState('conv-1', alice)

    setTurnExpandStorageScope('bob')
    const bob = emptyTurnExpandUiState()
    bob.expandedTurnIds.add('turn-bob')
    saveTurnExpandUiState('conv-1', bob)

    expect(loadTurnExpandUiState('conv-1').expandedTurnIds.has('turn-bob')).toBe(true)
    expect(loadTurnExpandUiState('conv-1').manuallyCollapsedTurnIds.has('turn-alice')).toBe(false)

    setTurnExpandStorageScope('alice')
    expect(loadTurnExpandUiState('conv-1').manuallyCollapsedTurnIds.has('turn-alice')).toBe(true)

    // Logout: clear memory + anon scope must not erase alice/bob buckets.
    clearAllTurnExpandUiState()
    setTurnExpandStorageScope(null)
    expect(getTurnExpandStorageScope()).toBe('anon')
    expect(localStorage.getItem(turnExpandStorageKey('alice'))).toBeTruthy()
    expect(localStorage.getItem(turnExpandStorageKey('bob'))).toBeTruthy()

    setTurnExpandStorageScope('alice')
    expect(loadTurnExpandUiState('conv-1').manuallyCollapsedTurnIds.has('turn-alice')).toBe(true)
  })

  it('migrates legacy unscoped blob into the active user scope once', () => {
    localStorage.setItem(
      TURN_EXPAND_LEGACY_STORAGE_KEY,
      JSON.stringify({
        v: 1,
        byConversation: {
          'conv-legacy': {
            expandedTurnIds: [],
            manuallyCollapsedTurnIds: ['old'],
            expandedChangeTurnIds: [],
            collapsedChangeTurnIds: [],
            updatedAt: 1
          }
        }
      })
    )
    setTurnExpandStorageScope('user-z')
    expect(localStorage.getItem(TURN_EXPAND_LEGACY_STORAGE_KEY)).toBeNull()
    expect(loadTurnExpandUiState('conv-legacy').manuallyCollapsedTurnIds.has('old')).toBe(true)
  })

  it('normalizes unsafe user id characters for storage keys', () => {
    expect(normalizeTurnExpandStorageScope('a/b c')).toBe('a_b_c')
    expect(normalizeTurnExpandStorageScope(null)).toBe('anon')
  })
})
