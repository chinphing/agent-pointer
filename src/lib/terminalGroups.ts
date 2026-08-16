import { createLeaf, type TerminalPaneNode } from './terminalLayout'

/**
 * 终端 Group（= tab）状态管理。
 *
 * VS Code 终端模型：tab = group，每个 group 持有一棵布局树（可含多个拆分
 * 窗格，每个叶子绑定一个 session）；拆分只扩充当前 group 的树、不产生新
 * tab；新建终端 = 新建 group + 全屏单窗格树；切 tab = 切换 group 并恢复
 * 该组自己的布局与焦点窗格。
 *
 * 本模块只管理 group 的集合/顺序/激活/焦点记忆，树的拆分合并仍由
 * terminalLayout 的不可变纯函数完成，调用方负责把新树写回 setGroupLayout。
 */

export interface TerminalGroup {
  id: string
  /** 该组当前的布局树；null = 空组（待删除）。 */
  layout: TerminalPaneNode | null
  /** 该组最后聚焦的窗格 id（切走再切回时恢复焦点）。 */
  focusedPaneId: string | null
}

export interface TerminalGroupsState {
  /** 按创建顺序排列的 group（tab 栏顺序）。 */
  groups: TerminalGroup[]
  /** 当前激活的 group id；null = 无任何 group。 */
  activeGroupId: string | null
}

export function createTerminalGroupsState(): TerminalGroupsState {
  return { groups: [], activeGroupId: null }
}

export function findGroup(state: TerminalGroupsState, groupId: string): TerminalGroup | null {
  return state.groups.find(group => group.id === groupId) ?? null
}

export function getActiveGroup(state: TerminalGroupsState): TerminalGroup | null {
  if (!state.activeGroupId) return null
  return findGroup(state, state.activeGroupId)
}

/**
 * 新建一个 group（单窗格绑定 sessionId），并激活它。
 * 返回 group id。
 */
export function addGroup(state: TerminalGroupsState, groupId: string, tabId: string | null): TerminalGroup {
  const group: TerminalGroup = {
    id: groupId,
    layout: createLeaf(tabId),
    focusedPaneId: null
  }
  state.groups.push(group)
  state.activeGroupId = groupId
  return group
}

/**
 * 把某个 group 设为激活；目标不存在时返回 false。
 * 会先记下原激活组的焦点窗格（调用方应传入原 focusedPaneId 以便恢复）。
 */
export function activateGroup(
  state: TerminalGroupsState,
  groupId: string,
  saveFromPaneId: string | null
): boolean {
  const from = getActiveGroup(state)
  if (from) from.focusedPaneId = saveFromPaneId
  const target = findGroup(state, groupId)
  if (!target) {
    state.activeGroupId = null
    return false
  }
  state.activeGroupId = groupId
  return true
}

/**
 * 删除一个 group；若删除的是激活组，自动切到相邻组（优先下一个，其次上一个）。
 * 返回删除后应激活的 group id（可能为 null = 无剩余 group）。
 */
export function removeGroup(state: TerminalGroupsState, groupId: string): string | null {
  const index = state.groups.findIndex(group => group.id === groupId)
  if (index < 0) return state.activeGroupId
  state.groups.splice(index, 1)
  if (state.activeGroupId !== groupId) return state.activeGroupId
  const next = state.groups[index] ?? state.groups[index - 1] ?? null
  state.activeGroupId = next?.id ?? null
  return state.activeGroupId
}

/** 更新某个 group 的布局树（拆分/关闭/重启后写回新树）。 */
export function setGroupLayout(state: TerminalGroupsState, groupId: string, layout: TerminalPaneNode): void {
  const group = findGroup(state, groupId)
  if (group) group.layout = layout
}

/** 记录某个 group 的焦点窗格。 */
export function setGroupFocusedPane(state: TerminalGroupsState, groupId: string, paneId: string | null): void {
  const group = findGroup(state, groupId)
  if (group) group.focusedPaneId = paneId
}
