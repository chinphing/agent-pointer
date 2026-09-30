import { describe, expect, it } from 'vitest'
import type { AgentTrace, ChatMessage } from '../types/chat'
import {
  SELF_FORK_LABEL_SUFFIX,
  buildSubAgentTraceTree,
  isSelfForkTrace,
  rootTracesOf,
  selfForkTraceLabel
} from './subAgentTraceTree'

function trace(overrides: Partial<AgentTrace> & { id: string }): AgentTrace {
  return {
    name: overrides.name ?? overrides.id,
    role: 'worker',
    status: 'running',
    ...overrides
  }
}

function scopedRow(
  id: string,
  ownerInstanceId: string,
  traces: AgentTrace[]
): ChatMessage {
  return {
    id,
    role: 'assistant',
    content: '',
    status: 'done',
    createdAt: 0,
    agentInstanceId: ownerInstanceId,
    agentTrace: traces
  }
}

describe('buildSubAgentTraceTree', () => {
  it('links a nested spawn to its issuing worker through parentTraceId', () => {
    const lead = trace({ id: 'instance-coder', depth: 1, agentId: 'coder' })
    const fork = trace({
      id: 'instance-fork',
      depth: 2,
      agentId: 'coder',
      delegation: 'self',
      parentTraceId: 'instance-coder'
    })
    const explore = trace({
      id: 'instance-explore',
      depth: 3,
      agentId: 'explore',
      delegation: 'registered',
      parentTraceId: 'instance-fork'
    })

    const tree = buildSubAgentTraceTree({
      leadTraces: [lead],
      scopedRows: [
        scopedRow('round-1', 'instance-coder', [fork]),
        scopedRow('round-2', 'instance-fork', [explore])
      ]
    })

    expect(tree.roots.map(t => t.id)).toEqual(['instance-coder'])
    expect(tree.childrenOf('instance-coder').map(t => t.id)).toEqual(['instance-fork'])
    expect(tree.childrenOf('instance-fork').map(t => t.id)).toEqual(['instance-explore'])
    expect(tree.childrenOf('instance-explore')).toEqual([])
    expect(tree.parentOf('instance-explore')?.id).toBe('instance-fork')
    expect(tree.all.map(t => t.id)).toEqual([
      'instance-coder',
      'instance-fork',
      'instance-explore'
    ])
  })

  it('falls back to the owning scoped row when parentTraceId is absent (legacy rows)', () => {
    const lead = trace({ id: 'instance-coder', depth: 1 })
    const legacyChild = trace({ id: 'instance-legacy', depth: 2 })

    const tree = buildSubAgentTraceTree({
      leadTraces: [lead],
      scopedRows: [scopedRow('round-legacy', 'instance-coder', [legacyChild])]
    })

    expect(tree.roots.map(t => t.id)).toEqual(['instance-coder'])
    expect(tree.childrenOf('instance-coder').map(t => t.id)).toEqual(['instance-legacy'])
  })

  it('keeps traces whose parent is not merged as roots so they still render', () => {
    const orphan = trace({
      id: 'instance-orphan',
      depth: 2,
      parentTraceId: 'instance-missing'
    })

    const tree = buildSubAgentTraceTree({
      scopedRows: [scopedRow('round-x', 'instance-missing', [orphan])]
    })

    expect(tree.roots.map(t => t.id)).toEqual(['instance-orphan'])
    expect(tree.childrenOf('instance-missing')).toEqual([])
  })

  it('dedupes the same trace id and keeps the lead-level copy first', () => {
    const lead = trace({ id: 'instance-coder', depth: 1 })
    const sameTrace = trace({ id: 'instance-coder', depth: 1, detail: 'row copy' })

    const tree = buildSubAgentTraceTree({
      leadTraces: [lead],
      scopedRows: [scopedRow('round-dup', 'instance-lead', [sameTrace])]
    })

    expect(tree.all).toHaveLength(1)
    expect(tree.all[0]).toBe(lead)
    expect(tree.roots).toHaveLength(1)
  })

  it('drops self-referencing / cyclic linkage instead of looping', () => {
    const selfParent = trace({ id: 'instance-a', depth: 1, parentTraceId: 'instance-a' })
    const cycleA = trace({ id: 'instance-b', depth: 2, parentTraceId: 'instance-c' })
    const cycleC = trace({ id: 'instance-c', depth: 2, parentTraceId: 'instance-b' })

    const tree = buildSubAgentTraceTree({
      leadTraces: [selfParent],
      scopedRows: [scopedRow('round-cycle', 'instance-a', [cycleA, cycleC])]
    })

    expect(tree.roots.map(t => t.id).sort()).toEqual(['instance-a', 'instance-b'])
    // The cycle edge that would have closed the loop is dropped; the rest still nests.
    expect(tree.childrenOf('instance-b').map(t => t.id)).toEqual(['instance-c'])
    expect(tree.childrenOf('instance-c')).toEqual([])
  })
})

describe('rootTracesOf', () => {
  it('keeps depth-1 traces and drops children whose parent is present', () => {
    const parent = trace({ id: 'instance-parent', depth: 1 })
    const child = trace({ id: 'instance-child', depth: 2, parentTraceId: 'instance-parent' })

    expect(rootTracesOf([parent, child]).map(t => t.id)).toEqual(['instance-parent'])
  })

  it('keeps traces with an unknown or missing parent (backward compatible)', () => {
    const legacy = trace({ id: 'instance-legacy', depth: 1 })
    const dangling = trace({ id: 'instance-dangling', depth: 2, parentTraceId: 'nope' })

    expect(rootTracesOf([legacy, dangling]).map(t => t.id)).toEqual([
      'instance-legacy',
      'instance-dangling'
    ])
    expect(rootTracesOf(undefined)).toEqual([])
  })
})

describe('self fork labelling', () => {
  it('detects only the self delegation kind', () => {
    expect(isSelfForkTrace({ delegation: 'self' })).toBe(true)
    expect(isSelfForkTrace({ delegation: ' registered ' })).toBe(false)
    expect(isSelfForkTrace({})).toBe(false)
    expect(isSelfForkTrace(null)).toBe(false)
  })

  it('suffixes a fork label and leaves registered workers alone', () => {
    expect(selfForkTraceLabel('coder', { delegation: 'self' })).toBe(
      `coder${SELF_FORK_LABEL_SUFFIX}`
    )
    expect(selfForkTraceLabel('coder', { delegation: 'registered' })).toBe('coder')
    expect(selfForkTraceLabel('coder', {})).toBe('coder')
    // Never double-suffix an already labelled fork.
    expect(
      selfForkTraceLabel(`coder${SELF_FORK_LABEL_SUFFIX}`, { delegation: 'self' })
    ).toBe(`coder${SELF_FORK_LABEL_SUFFIX}`)
    expect(selfForkTraceLabel('', { delegation: 'self' })).toBe('')
  })
})
