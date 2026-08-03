// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest'
import {
  clearLastConversationId,
  readLastConversationId,
  writeLastConversationId
} from './lastConversation'

const STORAGE_KEY = 'pointer.chat.lastConversationId'

describe('lastConversation', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('reads and writes the active conversation id', () => {
    expect(readLastConversationId()).toBeNull()
    writeLastConversationId('conv-a')
    expect(localStorage.getItem(STORAGE_KEY)).toBe('conv-a')
    expect(readLastConversationId()).toBe('conv-a')
  })

  it('ignores blank ids on write', () => {
    writeLastConversationId('   ')
    expect(readLastConversationId()).toBeNull()
  })

  it('clears only when the stored id matches', () => {
    writeLastConversationId('conv-a')
    clearLastConversationId('conv-b')
    expect(readLastConversationId()).toBe('conv-a')
    clearLastConversationId('conv-a')
    expect(readLastConversationId()).toBeNull()
  })

  it('clears unconditionally when no id is passed', () => {
    writeLastConversationId('conv-a')
    clearLastConversationId()
    expect(readLastConversationId()).toBeNull()
  })
})
