/**
 * 终端拆分布局（递归二分树）。
 *
 * 叶子 = 一个终端会话（tabId）；分支 = 水平/垂直分割，两个子节点。
 * 面板层用 MAX_PANES 限制叶子总数（拆分按钮在达到上限时禁用）。
 * 所有更新函数都返回新的不可变节点树，便于单测与 Vue 响应式替换。
 */
export type TerminalPaneDirection = 'row' | 'column'

export interface TerminalPaneLeaf {
  kind: 'leaf'
  id: string
  /** 绑定的 console 会话 id；null = 空窗格。 */
  tabId: string | null
}

export interface TerminalPaneBranch {
  kind: 'branch'
  id: string
  direction: TerminalPaneDirection
  /** 两个子窗格的 flex 占比（百分比，和约 100）。 */
  sizes: [number, number]
  children: [TerminalPaneNode, TerminalPaneNode]
}

export type TerminalPaneNode = TerminalPaneLeaf | TerminalPaneBranch

export const MAX_PANES = 4
/** 分割条拖拽时子窗格的最小占比（百分比）。 */
export const MIN_PANE_RATIO = 0.15

let paneSeq = 0
export function nextPaneId(): string {
  paneSeq += 1
  return `pane-${paneSeq}`
}

export function createLeaf(tabId: string | null): TerminalPaneLeaf {
  return { kind: 'leaf', id: nextPaneId(), tabId }
}

export function createBranch(
  direction: TerminalPaneDirection,
  left: TerminalPaneNode,
  right: TerminalPaneNode,
  sizes: [number, number] = [50, 50]
): TerminalPaneBranch {
  return { kind: 'branch', id: nextPaneId(), direction, sizes, children: [left, right] }
}

export function leafCount(node: TerminalPaneNode | null): number {
  if (!node) return 0
  if (node.kind === 'leaf') return 1
  return leafCount(node.children[0]) + leafCount(node.children[1])
}

export function collectLeaves(node: TerminalPaneNode | null): TerminalPaneLeaf[] {
  if (!node) return []
  if (node.kind === 'leaf') return [node]
  return [...collectLeaves(node.children[0]), ...collectLeaves(node.children[1])]
}

export function findLeaf(node: TerminalPaneNode | null, leafId: string): TerminalPaneLeaf | null {
  if (!node) return null
  if (node.kind === 'leaf') return node.id === leafId ? node : null
  return findLeaf(node.children[0], leafId) ?? findLeaf(node.children[1], leafId)
}

/** 查找绑定了 tabId 的叶子。 */
export function findLeafByTabId(node: TerminalPaneNode | null, tabId: string): TerminalPaneLeaf | null {
  return collectLeaves(node).find(leaf => leaf.tabId === tabId) ?? null
}

/** 查找第一个有绑定会话的叶子（拆分的默认目标）。 */
export function firstLeafWithTab(node: TerminalPaneNode | null): TerminalPaneLeaf | null {
  return collectLeaves(node).find(leaf => leaf.tabId !== null) ?? null
}

/**
 * 把 leafId 叶子替换为「原叶子 + 新空叶子」的分支。
 * 返回新树和新叶子（新叶子 tabId=null，由调用方绑定会话）。
 */
export function splitLeaf(
  node: TerminalPaneNode,
  leafId: string,
  direction: TerminalPaneDirection
): { node: TerminalPaneNode; newLeaf: TerminalPaneLeaf } {
  const newLeaf = createLeaf(null)
  const updated = replaceNode(node, leafId, leaf => createBranch(direction, leaf, newLeaf))
  return { node: updated, newLeaf }
}

function replaceNode(
  node: TerminalPaneNode,
  leafId: string,
  replacer: (leaf: TerminalPaneLeaf) => TerminalPaneNode
): TerminalPaneNode {
  if (node.kind === 'leaf') return node.id === leafId ? replacer(node) : node
  return {
    ...node,
    children: [replaceNode(node.children[0], leafId, replacer), replaceNode(node.children[1], leafId, replacer)]
  }
}

/**
 * 移除叶子；分支只剩一个子节点时折叠为子节点。
 * 返回新树（可能为 null）+ 被移除的叶子。
 */
export function removeLeaf(
  node: TerminalPaneNode,
  leafId: string
): { node: TerminalPaneNode | null; removed: TerminalPaneLeaf | null } {
  return removeFromNode(node, leafId)
}

function removeFromNode(
  node: TerminalPaneNode,
  leafId: string
): { node: TerminalPaneNode | null; removed: TerminalPaneLeaf | null } {
  if (node.kind === 'leaf') {
    if (node.id === leafId) return { node: null, removed: node }
    return { node, removed: null }
  }
  const left = removeFromNode(node.children[0], leafId)
  const right = removeFromNode(node.children[1], leafId)
  const removed = left.removed ?? right.removed
  if (!removed) return { node, removed: null }
  if (left.node && right.node) {
    return { node: { ...node, children: [left.node, right.node] }, removed }
  }
  // 某一侧被删 → 折叠为剩余子节点
  return { node: left.node ?? right.node, removed }
}

/** 把 leafId 叶子的 tabId 改为新值（找不到时原样返回）。 */
export function setLeafTabId(node: TerminalPaneNode, leafId: string, tabId: string | null): TerminalPaneNode {
  if (node.kind === 'leaf') {
    if (node.id !== leafId) return node
    return node.tabId === tabId ? node : { ...node, tabId }
  }
  const left = setLeafTabId(node.children[0], leafId, tabId)
  const right = setLeafTabId(node.children[1], leafId, tabId)
  if (left === node.children[0] && right === node.children[1]) return node
  return {
    ...node,
    children: [left, right]
  }
}
