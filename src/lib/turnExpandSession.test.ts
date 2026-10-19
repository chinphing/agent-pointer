// @vitest-environment happy-dom
/**
 * Regression: a keyed `MessageList` must persist expand overrides under the
 * conversation it was mounted for.
 *
 * `ChatView` renders `MessageList` with `:key="chat.currentId"`, and the chat
 * store switches `currentId` *before* Vue unmounts the keyed child. Reading
 * `chat.currentId` in `onBeforeUnmount` therefore wrote the outgoing snapshot
 * into the incoming conversation's bucket, so every switch copied the union of
 * everything seen so far into the next bucket (and then into disk).
 *
 * The harness below mirrors that order with the production session factory:
 * `currentId` changes, then the keyed child unmounts/mounts.
 */
import { beforeEach, describe, expect, it } from 'vitest'
import { createApp, defineComponent, h, nextTick, onBeforeUnmount, ref } from 'vue'
import {
  clearAllTurnExpandUiState,
  emptyTurnExpandUiState,
  loadTurnExpandUiState,
  resetTurnExpandUiStateForTests,
  saveTurnExpandUiState,
  setTurnExpandStorageScope,
  turnExpandStorageKey
} from './turnExpandState'
import { createTurnExpandSession } from './turnExpandSession'

type HarnessInstance = {
  expanded: Set<string>
  collapsed: Set<string>
  persist(): void
}

const currentId = ref<string | null>(null)
let instances: Record<string, HarnessInstance> = {}
let mounts: Array<{ id: string; expanded: string[]; collapsed: string[] }> = []

const HarnessMessageList = defineComponent({
  name: 'HarnessMessageList',
  setup() {
    // Same shape as MessageList.vue: the id is captured once, at mount.
    const session = createTurnExpandSession(currentId.value)
    const loaded = session.load()
    const expanded = new Set(loaded.expandedTurnIds)
    const collapsed = new Set(loaded.manuallyCollapsedTurnIds)
    mounts.push({ id: session.conversationId, expanded: [...expanded], collapsed: [...collapsed] })

    const persist = (): void => session.save({
      expandedTurnIds: expanded,
      manuallyCollapsedTurnIds: collapsed,
      expandedChangeTurnIds: new Set(),
      collapsedChangeTurnIds: new Set()
    })
    instances[session.conversationId] = { expanded, collapsed, persist }
    onBeforeUnmount(persist)
    return () => h('div')
  }
})

function mountList() {
  const host = document.createElement('div')
  document.body.appendChild(host)
  const app = createApp({
    render: () => h(HarnessMessageList, { key: currentId.value ?? 'none' })
  })
  app.mount(host)
  return app
}

async function switchTo(id: string): Promise<void> {
  currentId.value = id
  await nextTick()
}

function readDisk(): Record<string, { expandedTurnIds?: string[] }> {
  const raw = localStorage.getItem(turnExpandStorageKey('user-a'))
  const parsed = raw ? JSON.parse(raw) as { byConversation?: Record<string, { expandedTurnIds?: string[] }> } : {}
  return parsed.byConversation ?? {}
}

beforeEach(() => {
  resetTurnExpandUiStateForTests()
  setTurnExpandStorageScope('user-a')
  currentId.value = null
  instances = {}
  mounts = []
  document.body.innerHTML = ''
})

describe('turnExpandSession', () => {
  it('captures the mount id and ignores later store switches', () => {
    const session = createTurnExpandSession('  conv-a  ')
    expect(session.conversationId).toBe('conv-a')
    expect(createTurnExpandSession(null).conversationId).toBe('')
  })

  it('no-ops saves when the instance was mounted without a conversation id', () => {
    const session = createTurnExpandSession(null)
    const state = emptyTurnExpandUiState()
    state.expandedTurnIds.add('turn-a')
    session.save(state)
    expect(localStorage.getItem(turnExpandStorageKey('user-a'))).toBeNull()
  })

  it('does not write the outgoing snapshot under the incoming conversation id', async () => {
    currentId.value = 'A'
    const app = mountList()
    instances.A!.expanded.add('turn-a1')
    instances.A!.persist()

    // store switches first, Vue unmounts the keyed child afterwards
    await switchTo('B')

    expect(readDisk().B?.expandedTurnIds ?? []).not.toContain('turn-a1')
    expect(loadTurnExpandUiState('B').expandedTurnIds.has('turn-a1')).toBe(false)
    // the outgoing conversation keeps its own override
    expect(loadTurnExpandUiState('A').expandedTurnIds.has('turn-a1')).toBe(true)

    app.unmount()
  })

  it('keeps this conversation’s overrides after switching away and back', async () => {
    currentId.value = 'A'
    const app = mountList()
    instances.A!.collapsed.add('turn-a1')
    instances.A!.expanded.add('turn-a2')
    instances.A!.persist()

    await switchTo('B')
    instances.B!.expanded.add('turn-b1')
    instances.B!.persist()

    // Force the return trip to read disk instead of the in-memory cache.
    clearAllTurnExpandUiState()
    await switchTo('A')

    const back = instances.A!
    expect(back.collapsed.has('turn-a1')).toBe(true)
    expect(back.expanded.has('turn-a2')).toBe(true)
    expect(back.expanded.has('turn-b1')).toBe(false)

    app.unmount()
  })

  it('keeps every bucket to its own ids across a chain of switches', async () => {
    currentId.value = 'A'
    const app = mountList()
    instances.A!.expanded.add('a1')
    instances.A!.persist()
    await switchTo('B')
    instances.B!.expanded.add('b1')
    instances.B!.persist()
    await switchTo('C')
    instances.C!.expanded.add('c1')
    instances.C!.persist()

    clearAllTurnExpandUiState()
    await switchTo('A')

    expect([...instances.A!.expanded]).toEqual(['a1'])
    expect([...instances.A!.collapsed]).toEqual([])
    expect(mounts.map(m => m.id)).toEqual(['A', 'B', 'C', 'A'])
    expect(mounts[1]!.expanded).not.toContain('a1')
    expect(mounts[2]!.expanded).not.toContain('b1')

    app.unmount()
  })

  it('still shares one disk key per user scope', () => {
    const state = emptyTurnExpandUiState()
    state.manuallyCollapsedTurnIds.add('kept')
    saveTurnExpandUiState('conv-a', state)
    expect(readDisk()['conv-a']?.expandedTurnIds ?? []).toEqual([])
    expect(loadTurnExpandUiState('conv-a').manuallyCollapsedTurnIds.has('kept')).toBe(true)
  })
})
