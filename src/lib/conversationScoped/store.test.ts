import { describe, expect, it } from 'vitest'
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
})
