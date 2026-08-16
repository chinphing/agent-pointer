import { describe, expect, it } from 'vitest'
import {
  activateGroup,
  addGroup,
  createTerminalGroupsState,
  findGroup,
  getActiveGroup,
  removeGroup,
  setGroupFocusedPane,
  setGroupLayout
} from './terminalGroups'
import { collectLeaves, createLeaf, leafCount, setLeafTabId, splitLeaf } from './terminalLayout'

describe('terminalGroups', () => {
  it('addGroup appends a new group, activates it, and binds a single leaf', () => {
    const state = createTerminalGroupsState()
    const group = addGroup(state, 'g-1', 'tab-a')
    expect(state.groups).toHaveLength(1)
    expect(state.activeGroupId).toBe('g-1')
    expect(group.layout?.kind).toBe('leaf')
    expect(leafCount(group.layout)).toBe(1)
    expect(collectLeaves(group.layout!)[0]?.tabId).toBe('tab-a')
  })

  it('activateGroup remembers the previous group focused pane and switches active id', () => {
    const state = createTerminalGroupsState()
    addGroup(state, 'g-1', 'tab-a')
    setGroupFocusedPane(state, 'g-1', 'pane-1')
    addGroup(state, 'g-2', 'tab-b')

    const ok = activateGroup(state, 'g-1', 'pane-2')
    expect(ok).toBe(true)
    expect(state.activeGroupId).toBe('g-1')
    // 切走 g-2 时保存了它的焦点
    expect(findGroup(state, 'g-2')?.focusedPaneId).toBe('pane-2')
  })

  it('activateGroup with unknown id deactivates and returns false', () => {
    const state = createTerminalGroupsState()
    addGroup(state, 'g-1', 'tab-a')
    const ok = activateGroup(state, 'missing', null)
    expect(ok).toBe(false)
    expect(state.activeGroupId).toBeNull()
  })

  it('removeGroup removes the group; deleting active group picks next then previous', () => {
    const state = createTerminalGroupsState()
    addGroup(state, 'g-1', 'tab-a')
    addGroup(state, 'g-2', 'tab-b')
    addGroup(state, 'g-3', 'tab-c')

    // 删除中间组（非激活组）：激活组不变
    const next1 = removeGroup(state, 'g-2')
    expect(state.groups.map(g => g.id)).toEqual(['g-1', 'g-3'])
    expect(next1).toBe('g-3')

    // 删除激活组 g-3（末尾）→ 切到上一个 g-1
    const next2 = removeGroup(state, 'g-3')
    expect(state.activeGroupId).toBe('g-1')
    expect(next2).toBe('g-1')

    // 删除最后剩余 → null
    const next3 = removeGroup(state, 'g-1')
    expect(state.groups).toHaveLength(0)
    expect(next3).toBeNull()
    expect(state.activeGroupId).toBeNull()
  })

  it('removeGroup of unknown id is a no-op returning current active', () => {
    const state = createTerminalGroupsState()
    addGroup(state, 'g-1', 'tab-a')
    const next = removeGroup(state, 'missing')
    expect(next).toBe('g-1')
    expect(state.groups).toHaveLength(1)
  })

  it('setGroupLayout updates the group tree (split expansion stays inside the group)', () => {
    const state = createTerminalGroupsState()
    const group = addGroup(state, 'g-1', 'tab-a')
    const root = group.layout!
    const { node, newLeaf } = splitLeaf(root, root.id, 'row')
    const expanded = setLeafTabId(node, newLeaf.id, 'tab-b')
    setGroupLayout(state, 'g-1', expanded)

    const updated = findGroup(state, 'g-1')
    expect(leafCount(updated?.layout ?? null)).toBe(2)
    // 仍是同一个 group，没有新增 tab
    expect(state.groups).toHaveLength(1)
    expect(state.activeGroupId).toBe('g-1')
    expect(newLeaf.tabId).toBeNull()
  })

  it('getActiveGroup mirrors activeGroupId', () => {
    const state = createTerminalGroupsState()
    expect(getActiveGroup(state)).toBeNull()
    addGroup(state, 'g-1', 'tab-a')
    expect(getActiveGroup(state)?.id).toBe('g-1')
  })
})
