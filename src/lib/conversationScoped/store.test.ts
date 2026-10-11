import { describe, expect, it } from 'vitest'
import { effect, stop } from 'vue'
import type { ChatMessage } from '../../types/chat'
import { computeSubAgentLiveFingerprint } from './liveFingerprint'
import { resolveSpawnId } from './spawnId'
import { createConversationScopedStore } from './store'

function scopedMsg(over: Partial<ChatMessage> & { id: string }): ChatMessage {
  return {
    role: 'assistant',
    content: '',
    status: 'done',
    createdAt: 0,
    anchorMessageId: 'anchor-1',
    traceId: 'task:explore',
    ...over
  }
}

describe('resolveSpawnId', () => {
  it('prefers agentInstanceId', () => {
    expect(resolveSpawnId({
      agentInstanceId: 'inst-1',
      anchorMessageId: 'a',
      traceId: 'task:explore'
    })).toBe('inst-1')
  })

  it('reads instance from explore/self trace ids', () => {
    expect(resolveSpawnId({
      traceId: 'task:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee:explore'
    })).toBe('aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee')
  })

  it('treats colon-free ids as SpawnId instead of a composite key', () => {
    const spawn = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'
    expect(resolveSpawnId({
      anchorMessageId: 'lead',
      traceId: spawn
    })).toBe(spawn)
  })
})

describe('createConversationScopedStore', () => {
  it('writes rows off the lead timeline and looks them up by spawn', () => {
    const store = createConversationScopedStore()
    const row = scopedMsg({ id: 'm1', agentInstanceId: 'inst-1', content: 'hello' })
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      anchorMessageId: 'anchor-1',
      traceId: 'task:explore'
    }, row)
    expect(store.getRows('c1', { agentInstanceId: 'inst-1' }).map(m => m.id)).toEqual(['m1'])
    expect(store.findRow('c1', 'm1')?.content).toBe('hello')
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      anchorMessageId: 'anchor-1',
      traceId: 'task:explore'
    }, scopedMsg({ id: 'm2', agentInstanceId: 'inst-1', createdAt: 1 }))
    expect(store.findRow('c1', 'm2')?.id).toBe('m2')
    expect(store.findRow('c1', 'missing')).toBeUndefined()
    expect(store.listSpawnIdsForAnchors('c1', ['anchor-1'])).toEqual(['inst-1'])
    expect(store.getMembershipSignal('c1')).toBeGreaterThan(0)
    expect(store.getRows('c1', {
      anchorMessageId: 'anchor-1',
      traceId: 'inst-1'
    }).map(m => m.id)).toEqual(['m1', 'm2'])
    expect(store.listUnpersistedRows('c1', new Set(['m1', 'm2']))).toEqual([])
    expect(store.listUnpersistedRows('c1').map(m => m.id)).toEqual(['m1', 'm2'])
  })

  it('evicts rows and allows assignLoaded after stub', () => {
    const store = createConversationScopedStore()
    const row = scopedMsg({ id: 'm1', agentInstanceId: 'inst-1' })
    store.ensureInstance('c1', { agentInstanceId: 'inst-1', traceId: 't' }, row)
    store.evictInstance('c1', { agentInstanceId: 'inst-1' }, 'stub')
    expect(store.getRows('c1', { agentInstanceId: 'inst-1' })).toEqual([])
    expect(store.hasLoadedRows('c1', { agentInstanceId: 'inst-1' })).toBe(false)
    store.assignLoaded('c1', { agentInstanceId: 'inst-1', traceId: 't' }, [row])
    expect(store.hasLoadedRows('c1', { agentInstanceId: 'inst-1' })).toBe(true)
  })

  /**
   * Contract guard for the per-chunk path: `live` moves on tokens, `askUser` / `structure`
   * never do. Behaviour stays green even if a text chunk is routed back through the full
   * scan — the cost does not, and that cost is invisible to the HUD's `ms:` / `render:`
   * counters (it lands on JSC's GC threads). So read `call:scopedSignals` next to this
   * test: it must stay ≈0 while a sub-agent streams.
   *   full scan = 97.3 µs and 32.7 KB of strings per call
   *   delta     =  0.13 µs and 13 bytes
   */
  it('a text delta moves only the live length, never structure', () => {
    const store = createConversationScopedStore()
    const row = scopedMsg({ id: 'm1', agentInstanceId: 'inst-1', content: 'a' })
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      traceId: 'task:explore',
      anchorMessageId: 'anchor-1'
    }, row)
    const liveBefore = store.getLiveSignal('c1', 'inst-1')
    const structureBefore = store.getStructureRevision('c1')

    // A streamed text append takes the O(1) path: no scan, no signature rebuild.
    row.content = 'abc'
    store.touchTextDelta('c1', 'm1', 2)
    expect(store.getLiveSignal('c1', 'inst-1')).not.toBe(liveBefore)
    // `structure` (and `askUser`, which shares this path) must not move on a token write.
    expect(store.getStructureRevision('c1')).toBe(structureBefore)

    // …but a real shape change still does: the O(1) path is not a stale cache.
    row.status = 'streaming'
    store.touchRow('c1', 'm1')
    expect(store.getStructureRevision('c1')).not.toBe(structureBefore)
  })

  /**
   * The O(1) lane is only sound while its two accumulators agree with the scan they
   * replace. This pins that: after a mixed run of text and tool-payload appends, the
   * cached `live` must be byte-identical to a fresh `computeScopedSignals(...).live`.
   * Drift here is a **frozen live line**, not a slow one.
   */
  it('O(1) deltas stay byte-identical to a full rescan', () => {
    const store = createConversationScopedStore()
    const row = scopedMsg({
      id: 'm1',
      agentInstanceId: 'inst-1',
      status: 'streaming',
      content: 'a',
      toolCalls: [{ id: 'tc1', name: 'terminal', status: 'running', arguments: '{}', result: 'out' }]
    })
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      traceId: 'task:explore',
      anchorMessageId: 'anchor-1'
    }, row)

    const argsChunk = '{"cmd":"ls"}'
    const outputChunk = 'more output'
    const rawChunk = 'raw chunk'

    row.content += 'bc'
    store.touchTextDelta('c1', 'm1', 2)
    row.toolCalls![0]!.arguments += argsChunk
    store.touchTextDelta('c1', 'm1', 0, { payloadDelta: argsChunk.length })
    row.toolCalls![0]!.result += outputChunk
    store.touchTextDelta('c1', 'm1', 0, { payloadDelta: outputChunk.length })
    row.rawContent = rawChunk
    store.touchTextDelta('c1', 'm1', rawChunk.length)

    expect(store.getLiveSignal('c1', 'inst-1')).toBe(
      computeSubAgentLiveFingerprint(store.getRows('c1', { agentInstanceId: 'inst-1' }))
    )
  })

  it('touchRow publishes live signal under instance and legacy trace id', () => {
    const store = createConversationScopedStore()
    const row = scopedMsg({ id: 'm1', agentInstanceId: 'inst-1', content: 'a' })
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      traceId: 'task:explore',
      anchorMessageId: 'anchor-1'
    }, row)
    const before = store.getLiveSignal('c1', 'task:explore')
    expect(before).toContain('|')
    const membershipBefore = store.getMembershipSignal('c1')
    row.content = 'ab'
    store.touchRow('c1', 'm1')
    expect(store.getMembershipSignal('c1')).toBe(membershipBefore)
    expect(store.getLiveSignal('c1', 'task:explore')).not.toBe(before)
    expect(store.getLiveSignal('c1', 'inst-1')).toBe(store.getLiveSignal('c1', 'task:explore'))
  })

  it('takeScopedFromMessages splits mixed hydrate pages', () => {
    const store = createConversationScopedStore()
    const lead: ChatMessage = {
      id: 'lead',
      role: 'assistant',
      content: 'hi',
      status: 'done',
      createdAt: 0
    }
    const child = scopedMsg({ id: 'm1', agentInstanceId: 'inst-1' })
    const next = store.takeScopedFromMessages('c1', [lead, child])
    expect(next.map(m => m.id)).toEqual(['lead'])
    expect(store.getRows('c1', { agentInstanceId: 'inst-1' }).map(m => m.id)).toEqual(['m1'])
  })

  it('ask_user revision ignores text growth and bumps on ask_user changes', () => {
    const store = createConversationScopedStore()
    const row = scopedMsg({
      id: 'm1',
      agentInstanceId: 'inst-1',
      status: 'streaming',
      toolCalls: [
        { id: 'tc-ask', name: 'ask_user', status: 'pending', arguments: '{"question":"q"' },
        { id: 'tc-read', name: 'read', status: 'running', arguments: '{}' }
      ]
    })
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      traceId: 'task:explore',
      anchorMessageId: 'anchor-1'
    }, row)
    const afterSpawn = store.getAskUserRevision('c1')
    expect(afterSpawn).toBeGreaterThan(0)
    expect(store.getAskUserRevision('other')).toBe(0)

    // Streamed text / non-ask_user tool traffic: the banner must not rescan.
    row.content = 'streamed body'
    row.toolCalls![1]!.result = 'x'.repeat(400)
    row.toolCalls![0]!.result = JSON.stringify({ selected: ['A'] })
    store.touchRow('c1', 'm1')
    expect(store.getAskUserRevision('c1')).toBe(afterSpawn)

    // ask_user arguments keep streaming → the banner must re-render the question.
    row.toolCalls![0]!.arguments += ',"options":[]}'
    store.touchRow('c1', 'm1')
    expect(store.getAskUserRevision('c1')).toBe(afterSpawn + 1)

    // Status change → the banner must rescan.
    row.toolCalls![0]!.status = 'success'
    store.touchRow('c1', 'm1')
    expect(store.getAskUserRevision('c1')).toBe(afterSpawn + 2)
  })

  /**
   * The per-turn spawn-plan guard reads one revision per anchor. It must move for the
   * anchor a shape event belongs to — and, for a **nested** spawn, for every ancestor
   * anchor up to the turn message, because the turn's closure reaches those rows through
   * its parent. A revision that only moved the leaf would leave the owning turn's plan
   * frozen.
   */
  it('bumps the anchor structure revision for the changed spawn and its ancestors', () => {
    const store = createConversationScopedStore()
    // lead turn message `anchor-1` → worker `inst-1` (rows r1)
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      anchorMessageId: 'anchor-1',
      traceId: 'task:explore'
    }, scopedMsg({ id: 'r1', agentInstanceId: 'inst-1', createdAt: 0 }))
    // nested spawn issued by that worker: its rows are anchored on the parent's own row `r1`
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-2',
      anchorMessageId: 'r1',
      traceId: 'task:explore'
    }, scopedMsg({ id: 'r2', agentInstanceId: 'inst-2', createdAt: 1, anchorMessageId: 'r1' }))
    // an unrelated turn, so the "only the changed branch moves" half is testable
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-3',
      anchorMessageId: 'anchor-9',
      traceId: 'task:explore'
    }, scopedMsg({ id: 'r9', agentInstanceId: 'inst-3', anchorMessageId: 'anchor-9' }))

    const turnBefore = store.getAnchorStructureRevision('c1', 'anchor-1')
    const midBefore = store.getAnchorStructureRevision('c1', 'r1')
    const otherBefore = store.getAnchorStructureRevision('c1', 'anchor-9')

    // A shape change in the nested spawn must reach the turn anchor too: the turn's
    // closure only gets to `r2` through `r1`, which only hangs off `anchor-1`.
    const leafRow = store.findRow('c1', 'r2')!
    leafRow.status = 'streaming'
    store.touchRow('c1', 'r2')

    expect(store.getAnchorStructureRevision('c1', 'r1')).toBeGreaterThan(midBefore)
    expect(store.getAnchorStructureRevision('c1', 'anchor-1')).toBeGreaterThan(turnBefore)
    // The sibling turn is untouched — this is the whole point of the per-anchor axis.
    expect(store.getAnchorStructureRevision('c1', 'anchor-9')).toBe(otherBefore)

    // A shape change on the first level moves the same turn anchor and no one else.
    const rootRow = store.findRow('c1', 'r1')!
    rootRow.status = 'streaming'
    store.touchRow('c1', 'r1')
    expect(store.getAnchorStructureRevision('c1', 'anchor-1')).toBeGreaterThan(turnBefore + 1)
    expect(store.getAnchorStructureRevision('c1', 'anchor-9')).toBe(otherBefore)
  })

  it('a token write never moves the anchor structure revision', () => {
    const store = createConversationScopedStore()
    const row = scopedMsg({ id: 'm1', agentInstanceId: 'inst-1', content: 'a' })
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      traceId: 'task:explore',
      anchorMessageId: 'anchor-1'
    }, row)
    const before = store.getAnchorStructureRevision('c1', 'anchor-1')
    row.content = 'abcdef'
    store.touchTextDelta('c1', 'm1', 5)
    expect(store.getAnchorStructureRevision('c1', 'anchor-1')).toBe(before)
  })

  it('an anchor-less shape event falls back to the anchorless epoch', () => {
    const store = createConversationScopedStore()
    const before = store.getAnchorlessStructureEpoch()
    store.bumpStructureRevision('c1')
    expect(store.getAnchorlessStructureEpoch()).toBeGreaterThan(before)
    // With an anchor, the epoch stays put and only that anchor moves.
    const epochAfter = store.getAnchorlessStructureEpoch()
    const anchorBefore = store.getAnchorStructureRevision('c1', 'anchor-1')
    store.bumpStructureRevision('c1', 'anchor-1')
    expect(store.getAnchorlessStructureEpoch()).toBe(epochAfter)
    expect(store.getAnchorStructureRevision('c1', 'anchor-1')).toBeGreaterThan(anchorBefore)
  })

  /**
   * The per-turn spawn-plan pass short-cuts to an O(1) check when the conversation-level
   * scoped counter has not moved: it then judges a turn by object identity plus its
   * collapse projection alone, skipping every per-anchor revision read. That is only sound
   * while the anchor axis cannot move on its own — so every shape bump has to raise both,
   * and a token write (the highest-frequency event of all) has to raise neither.
   */
  it('keeps the conversation counter and the anchor axis moving together', () => {
    const store = createConversationScopedStore()
    store.ensureInstance(
      'c1',
      { agentInstanceId: 'inst-1', traceId: 'task:explore', anchorMessageId: 'anchor-1' },
      scopedMsg({ id: 'm1', agentInstanceId: 'inst-1', content: 'a' })
    )

    const convBefore = store.getStructureRevision('c1')
    const anchorBefore = store.getAnchorStructureRevision('c1', 'anchor-1')
    const epochBefore = store.getAnchorlessStructureEpoch()

    // Token growth is not a shape change: neither axis may move, or the pass would lose
    // its short cut on every streamed chunk.
    const row = store.findRow('c1', 'm1')!
    row.content = 'abcdef'
    store.touchTextDelta('c1', 'm1', 5)
    expect(store.getStructureRevision('c1')).toBe(convBefore)
    expect(store.getAnchorStructureRevision('c1', 'anchor-1')).toBe(anchorBefore)

    // A shape change moves the anchor axis and the conversation counter together, and
    // leaves the anchor-less epoch alone because the anchor resolved.
    row.status = 'streaming'
    store.touchRow('c1', 'm1')
    expect(store.getAnchorStructureRevision('c1', 'anchor-1')).toBeGreaterThan(anchorBefore)
    expect(store.getStructureRevision('c1')).toBeGreaterThan(convBefore)
    expect(store.getAnchorlessStructureEpoch()).toBe(epochBefore)
  })

  it('ensureInstance with empty lookup warns and skips', () => {
    const store = createConversationScopedStore()
    expect(store.ensureInstance('c1', {}, scopedMsg({ id: 'm1' }))).toBeUndefined()
    expect(store.findRow('c1', 'm1')).toBeUndefined()
  })

  it('assignLoaded does not replace live streaming row objects', () => {
    const store = createConversationScopedStore()
    const live = scopedMsg({ id: 'm1', agentInstanceId: 'inst-1', content: 'live' })
    store.ensureInstance('c1', { agentInstanceId: 'inst-1', traceId: 't' }, live)
    live.content = 'streamed'
    const stale = scopedMsg({ id: 'm1', agentInstanceId: 'inst-1', content: 'db-old' })
    const extra = scopedMsg({ id: 'm2', agentInstanceId: 'inst-1', content: 'from-db' })
    store.assignLoaded('c1', { agentInstanceId: 'inst-1', traceId: 't' }, [stale, extra])
    const rows = store.getRows('c1', { agentInstanceId: 'inst-1' })
    expect(rows.find(m => m.id === 'm1')).toBe(live)
    expect(live.content).toBe('streamed')
    expect(rows.find(m => m.id === 'm2')?.content).toBe('from-db')
    expect(store.getTranscript('c1', { agentInstanceId: 'inst-1' })?.loadState).toBe('streaming')
  })

  it('legacy trace lookup prefers the latest spawn', () => {
    const store = createConversationScopedStore()
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-old',
      anchorMessageId: 'a',
      traceId: 'task:coder'
    }, scopedMsg({ id: 'm-old', agentInstanceId: 'inst-old', traceId: 'task:coder' }))
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-new',
      anchorMessageId: 'a',
      traceId: 'task:coder'
    }, scopedMsg({ id: 'm-new', agentInstanceId: 'inst-new', traceId: 'task:coder' }))
    expect(store.getTranscript('c1', { anchorMessageId: 'a', traceId: 'task:coder' })?.instanceId)
      .toBe('inst-new')
  })

  it('evictInstance removes the bucket and nested child spawns', () => {
    const store = createConversationScopedStore()
    store.ensureInstance('c1', {
      agentInstanceId: 'parent',
      anchorMessageId: 'lead'
    }, scopedMsg({ id: 'parent-row', agentInstanceId: 'parent', anchorMessageId: 'lead' }))
    store.ensureInstance('c1', {
      agentInstanceId: 'child',
      anchorMessageId: 'parent-row'
    }, scopedMsg({ id: 'child-row', agentInstanceId: 'child', anchorMessageId: 'parent-row' }))
    store.evictInstance('c1', { agentInstanceId: 'parent' }, 'stub')
    expect(store.getTranscript('c1', { agentInstanceId: 'parent' })).toBeUndefined()
    expect(store.getRows('c1', { agentInstanceId: 'child' })).toEqual([])
    expect(store.getLiveSignal('c1', 'parent')).toBe('')
  })

  it('evictForRemovedMessages drops instances bound to trimmed anchors', () => {
    const store = createConversationScopedStore()
    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      anchorMessageId: 'lead'
    }, scopedMsg({ id: 'm1', agentInstanceId: 'inst-1', anchorMessageId: 'lead' }))
    store.evictForRemovedMessages('c1', [{
      id: 'lead',
      role: 'assistant',
      content: '',
      status: 'done',
      createdAt: 0
    }])
    expect(store.getRows('c1', { agentInstanceId: 'inst-1' })).toEqual([])
  })

  it('isScopedMessageId answers globally, conservatively, and releases', () => {
    const store = createConversationScopedStore()
    // Conservative default: unknown / empty ids are lead rows (fast lane).
    expect(store.isScopedMessageId('m1')).toBe(false)
    expect(store.isScopedMessageId('')).toBe(false)
    expect(store.isScopedMessageId(null)).toBe(false)
    expect(store.isScopedMessageId(undefined)).toBe(false)

    store.ensureInstance('c1', {
      agentInstanceId: 'inst-1',
      anchorMessageId: 'anchor-1'
    }, scopedMsg({ id: 'm1', agentInstanceId: 'inst-1' }))
    expect(store.isScopedMessageId('m1')).toBe(true)
    expect(store.isScopedMessageId(' m1 ')).toBe(true)

    // Global: another conversation's scoped row is just as visible.
    store.ensureInstance('c2', {
      agentInstanceId: 'inst-2',
      anchorMessageId: 'anchor-2'
    }, scopedMsg({ id: 'm2', agentInstanceId: 'inst-2', anchorMessageId: 'anchor-2' }))
    expect(store.isScopedMessageId('m2')).toBe(true)

    // A row without an anchor is not a scoped row — it must stay on the fast lane.
    store.ensureInstance('c1', { agentInstanceId: 'inst-3' }, {
      id: 'lead-row',
      role: 'assistant',
      content: '',
      status: 'streaming',
      createdAt: 0
    })
    expect(store.isScopedMessageId('lead-row')).toBe(false)

    // Eviction releases the id (no only-grow leak).
    store.evictInstance('c1', { agentInstanceId: 'inst-1' }, 'stub')
    expect(store.isScopedMessageId('m1')).toBe(false)

    // clearConversation releases that conversation's ids only.
    store.clearConversation('c2')
    expect(store.isScopedMessageId('m2')).toBe(false)

    // clearAll wipes the index.
    store.ensureInstance('c3', {
      agentInstanceId: 'inst-4',
      anchorMessageId: 'anchor-4'
    }, scopedMsg({ id: 'm4', agentInstanceId: 'inst-4', anchorMessageId: 'anchor-4' }))
    expect(store.isScopedMessageId('m4')).toBe(true)
    store.clearAll()
    expect(store.isScopedMessageId('m4')).toBe(false)
  })

  // --- Regression: publishLive writes the inner spawn slot only -------------

  it('keeps a spawn write from invalidating readers of its siblings', () => {
    const store = createConversationScopedStore()
    const rowA = scopedMsg({ id: 'a1', agentInstanceId: 'inst-a', anchorMessageId: 'anchor-1', content: 'a' })
    const rowB = scopedMsg({ id: 'b1', agentInstanceId: 'inst-b', anchorMessageId: 'anchor-1', content: 'b' })
    store.ensureInstance('c1', { agentInstanceId: 'inst-a', anchorMessageId: 'anchor-1' }, rowA)
    store.ensureInstance('c1', { agentInstanceId: 'inst-b', anchorMessageId: 'anchor-1' }, rowB)

    let runsA = 0
    let runsBlank = 0
    const runnerA = effect(() => { store.getLiveSignal('c1', 'inst-a'); runsA += 1 })
    const runnerBlank = effect(() => { store.getLiveSignal('c1', ''); runsBlank += 1 })
    expect(runsA).toBe(1)
    expect(runsBlank).toBe(1)

    rowB.content = 'b grows'
    store.touchRow('c1', 'b1')
    expect(runsA).toBe(1) // sibling spawn write must not re-run A's reader
    expect(runsBlank).toBe(1) // the blank key no longer doubles as "any spawn moved"

    rowA.content = 'a grows'
    store.touchRow('c1', 'a1')
    expect(runsA).toBe(2) // A's own write still lands
    stop(runnerA)
    stop(runnerBlank)
  })

  it('releases the evicted spawn slot without leaking its subscriber', () => {
    const store = createConversationScopedStore()
    const rowA = scopedMsg({ id: 'a1', agentInstanceId: 'inst-a', anchorMessageId: 'anchor-1', content: 'a' })
    const rowB = scopedMsg({ id: 'b1', agentInstanceId: 'inst-b', anchorMessageId: 'anchor-1', content: 'b' })
    store.ensureInstance('c1', { agentInstanceId: 'inst-a', anchorMessageId: 'anchor-1' }, rowA)
    store.ensureInstance('c1', { agentInstanceId: 'inst-b', anchorMessageId: 'anchor-1' }, rowB)

    let runsA = 0
    const runnerA = effect(() => { store.getLiveSignal('c1', 'inst-a'); runsA += 1 })
    expect(runsA).toBe(1)
    expect(store.getLiveSignal('c1', 'inst-a')).not.toBe('')

    store.evictInstance('c1', { agentInstanceId: 'inst-a' }, 'stub')
    expect(runsA).toBe(2) // eviction is a real change: the reader must be released
    expect(store.getLiveSignal('c1', 'inst-a')).toBe('')

    rowB.content = 'b grows'
    store.touchRow('c1', 'b1')
    expect(runsA).toBe(2) // no leak: a later sibling write must not reach it
    stop(runnerA)
  })

  // --- Regression: the list still refreshes on shape changes ----------------

  it('structure revision tracks shape changes but never text growth', () => {
    const store = createConversationScopedStore()
    const row = scopedMsg({
      id: 'm1',
      agentInstanceId: 'inst-1',
      anchorMessageId: 'anchor-1',
      status: 'streaming',
      toolCalls: [{ id: 'tc-read', name: 'read', status: 'running', arguments: '{}' }]
    })
    store.ensureInstance('c1', { agentInstanceId: 'inst-1', anchorMessageId: 'anchor-1' }, row)
    const afterSpawn = store.getStructureRevision('c1')
    expect(afterSpawn).toBeGreaterThan(0)
    expect(store.getStructureRevision('other')).toBe(0)

    let runs = 0
    const runner = effect(() => { store.getStructureRevision('c1'); runs += 1 })
    expect(runs).toBe(1)

    // Pure text / payload growth: the turn spawn plans must not rebuild.
    row.content = 'streamed body '.repeat(20)
    row.thoughts = 'reasoning text'
    row.toolCalls![0]!.result = 'x'.repeat(500)
    store.touchRow('c1', 'm1')
    expect(store.getStructureRevision('c1')).toBe(afterSpawn)
    expect(runs).toBe(1)

    // Tool status change is a shape change: the list must refresh, never stay stale.
    row.toolCalls![0]!.status = 'success'
    store.touchRow('c1', 'm1')
    expect(store.getStructureRevision('c1')).toBe(afterSpawn + 1)
    expect(runs).toBe(2)

    // A new spawn is a shape change even before any row lands.
    const afterRowBump = store.getStructureRevision('c1')
    store.ensureInstance('c1', { agentInstanceId: 'inst-2', anchorMessageId: 'anchor-1' },
      scopedMsg({ id: 'm2', agentInstanceId: 'inst-2' }))
    expect(store.getStructureRevision('c1')).toBeGreaterThan(afterRowBump)

    // Eviction is a shape change too.
    const beforeEvict = store.getStructureRevision('c1')
    store.evictInstance('c1', { agentInstanceId: 'inst-2' }, 'stub')
    expect(store.getStructureRevision('c1')).toBe(beforeEvict + 1)
    stop(runner)
  })
})

describe('computeSubAgentLiveFingerprint', () => {
  it('tracks text and tool status', () => {
    const scoped = [
      scopedMsg({
        id: 'm1',
        content: 'hello',
        toolCalls: [{ id: 'tc1', name: 'read', arguments: '{}', status: 'running' }]
      })
    ]
    const fp1 = computeSubAgentLiveFingerprint(scoped)
    scoped[0].content = 'hello world'
    const fp2 = computeSubAgentLiveFingerprint(scoped)
    expect(fp1).not.toBe(fp2)
    scoped[0].toolCalls![0].status = 'success'
    const fp3 = computeSubAgentLiveFingerprint(scoped)
    expect(fp2).not.toBe(fp3)
  })

  it('tracks tool label/summary and argument length so live text can refresh', () => {
    const scoped = [
      scopedMsg({
        id: 'm1',
        toolCalls: [{
          id: 'tc1',
          name: 'media_understand',
          arguments: '{}',
          status: 'running',
          displayLabel: '媒体理解'
        }]
      })
    ]
    const fp1 = computeSubAgentLiveFingerprint(scoped)
    scoped[0].toolCalls![0].arguments = JSON.stringify({ goal: '识别发票' })
    expect(computeSubAgentLiveFingerprint(scoped)).not.toBe(fp1)
    scoped[0].toolCalls![0].displaySummary = '识别发票'
    expect(computeSubAgentLiveFingerprint(scoped)).not.toBe(fp1)
  })
})
