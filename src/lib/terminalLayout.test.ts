import { describe, expect, it } from 'vitest'
import {
  collectLeaves,
  createBranch,
  createLeaf,
  findLeaf,
  findLeafByTabId,
  firstLeafWithTab,
  leafCount,
  MAX_PANES,
  removeLeaf,
  setLeafTabId,
  splitLeaf,
  type TerminalPaneNode
} from './terminalLayout'

describe('terminalLayout', () => {
  it('createLeaf / leafCount on single node', () => {
    const leaf = createLeaf('tab-1')
    expect(leaf.kind).toBe('leaf')
    expect(leaf.tabId).toBe('tab-1')
    expect(leafCount(leaf)).toBe(1)
    expect(leafCount(null)).toBe(0)
  })

  it('splitLeaf replaces a leaf with a branch keeping the original tab', () => {
    const root = createLeaf('tab-a')
    const { node, newLeaf } = splitLeaf(root, root.id, 'row')
    expect(node.kind).toBe('branch')
    if (node.kind !== 'branch') return
    expect(node.direction).toBe('row')
    expect(node.children[0]).toMatchObject({ kind: 'leaf', tabId: 'tab-a' })
    expect(node.children[1]).toBe(newLeaf)
    expect(newLeaf.tabId).toBeNull()
    expect(leafCount(node)).toBe(2)
  })

  it('splitLeaf nests recursively and keeps ids stable', () => {
    const root = createLeaf('tab-a')
    const first = splitLeaf(root, root.id, 'row')
    const second = splitLeaf(first.node, first.newLeaf.id, 'column')
    expect(leafCount(second.node)).toBe(3)
    // original leaf id must survive the split
    expect(findLeaf(second.node, root.id)?.tabId).toBe('tab-a')
  })

  it('removeLeaf removes a leaf and collapses single-child branches', () => {
    const root = createLeaf('tab-a')
    const { node, newLeaf } = splitLeaf(root, root.id, 'row')
    const removed = removeLeaf(node, newLeaf.id)
    expect(removed.removed?.id).toBe(newLeaf.id)
    // branch with one child collapses back to a leaf
    expect(removed.node?.kind).toBe('leaf')
    expect(removed.node && 'tabId' in removed.node ? removed.node.tabId : null).toBe('tab-a')
    expect(leafCount(removed.node)).toBe(1)
  })

  it('removeLeaf of the last leaf returns null node', () => {
    const root = createLeaf('tab-a')
    const { node, removed } = removeLeaf(root, root.id)
    expect(removed?.tabId).toBe('tab-a')
    expect(node).toBeNull()
  })

  it('removeLeaf in a deeper tree collapses the affected branch only', () => {
    let root: TerminalPaneNode = createLeaf('tab-a')
    const first = splitLeaf(root, root.id, 'row') // [a, new1]
    root = first.node
    const second = splitLeaf(root, first.newLeaf.id, 'column') // [a, [new1, new2]]
    expect(leafCount(second.node)).toBe(3)
    const secondNode = second.node
    if (secondNode.kind !== 'branch') return
    const left = secondNode.children[0]
    const right = secondNode.children[1]
    if (left.kind !== 'leaf' || right.kind !== 'branch') return
    const removed = removeLeaf(second.node, right.children[1].id)
    // right branch collapses to its surviving child, left untouched
    expect(leafCount(removed.node)).toBe(2)
    expect(findLeaf(removed.node, left.id)?.tabId).toBe('tab-a')
    expect(removed.node?.kind).toBe('branch')
  })

  it('setLeafTabId updates only the target leaf', () => {
    const root = createLeaf('tab-a')
    const { node, newLeaf } = splitLeaf(root, root.id, 'column')
    const updated = setLeafTabId(node, newLeaf.id, 'tab-b')
    expect(findLeafByTabId(updated, 'tab-b')?.id).toBe(newLeaf.id)
    expect(findLeaf(updated, root.id)?.tabId).toBe('tab-a')
    // unknown leaf id → unchanged tree (same references)
    expect(setLeafTabId(updated, 'nope', 'tab-x')).toBe(updated)
  })

  it('findLeafByTabId / firstLeafWithTab / collectLeaves', () => {
    const root = createLeaf('tab-a')
    const { node } = splitLeaf(root, root.id, 'row')
    const updated = setLeafTabId(node, collectLeaves(node)[1].id, 'tab-b')
    expect(findLeafByTabId(updated, 'tab-a')?.id).toBe(root.id)
    expect(findLeafByTabId(updated, 'tab-b')?.id).toBe(collectLeaves(node)[1].id)
    expect(findLeafByTabId(updated, 'missing')).toBeNull()
    expect(firstLeafWithTab(updated)?.tabId).toBe('tab-a')
    expect(firstLeafWithTab(createLeaf(null))).toBeNull()
    expect(collectLeaves(updated).map(l => l.tabId)).toEqual(['tab-a', 'tab-b'])
  })

  it('createBranch keeps explicit sizes', () => {
    const branch = createBranch('column', createLeaf(null), createLeaf(null), [70, 30])
    expect(branch.sizes).toEqual([70, 30])
    expect(leafCount(branch)).toBe(2)
  })

  it('MAX_PANES is four', () => {
    expect(MAX_PANES).toBe(4)
  })
})
