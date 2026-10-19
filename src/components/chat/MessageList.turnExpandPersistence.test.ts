import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

/**
 * `MessageList.vue` is only reachable through the app shell — no fixture mounts
 * it — so this guards the persistence contract in its source, like
 * `MessageList.transcriptFlexSpace.test.ts` does for the markup.
 *
 * The contract: one instance = one conversation (`ChatView` keys it by
 * `chat.currentId`), so expand overrides are read/written through the id
 * captured at mount. Re-reading `chat.currentId` on unmount persisted the
 * outgoing snapshot under the conversation being opened.
 */
const source = readFileSync(new URL('./MessageList.vue', import.meta.url), 'utf8')

/** Comments explain the contract in prose; strip them so wording cannot fail it. */
const code = source
  .replace(/<!--[\s\S]*?-->/g, '')
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .replace(/^\s*\/\/.*$/gm, '')

describe('MessageList turn-expand persistence', () => {
  it('captures the conversation id once, at mount', () => {
    expect(code).toContain('createTurnExpandSession(chat.currentId)')
  })

  it('never reads or writes the live conversation id for expand state', () => {
    // Direct store calls are what leaked one conversation's snapshot into the next.
    expect(code).not.toContain('saveTurnExpandUiState')
    expect(code).not.toContain('loadTurnExpandUiState')
    expect(code).not.toMatch(/turnExpandSession\.(?:save|load)\([^)]*chat\.currentId/)
  })

  it('loads once at mount instead of watching chat.currentId', () => {
    expect(code).toContain('applyTurnExpandUiState(turnExpandSession.load())')
    // The old watcher's cross-conversation branch, dead under `:key`.
    expect(code).not.toContain('prev?.trim()')
  })

  it('persists the outgoing snapshot on unmount', () => {
    expect(code).toContain('turnExpandSession.save(snapshotTurnExpandUiState())')
  })
})
