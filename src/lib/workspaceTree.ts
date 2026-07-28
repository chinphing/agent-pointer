import type { WorkspaceEntry } from './api'

export type WorkspaceTreeNodeModel = WorkspaceEntry & {
  children?: WorkspaceTreeNodeModel[]
  expanded?: boolean
  loading?: boolean
}

/** Merge a fresh directory listing onto an existing tree, keeping expanded subtrees. */
export function mergeWorkspaceTreePreserveState(
  previous: WorkspaceTreeNodeModel[],
  next: WorkspaceEntry[]
): WorkspaceTreeNodeModel[] {
  const prevByPath = new Map(previous.map(node => [node.path, node]))
  return next.map(entry => {
    const old = prevByPath.get(entry.path)
    if (!old) return { ...entry }
    const merged: WorkspaceTreeNodeModel = {
      ...entry,
      expanded: old.expanded === true,
      loading: false
    }
    if (entry.kind === 'directory' && old.children) {
      merged.children = old.children
    }
    return merged
  })
}

/** Parent directory prefixes for `a/b/c.txt` → `['a', 'a/b']`. */
export function workspacePathAncestorDirs(relativePath: string): string[] {
  const parts = relativePath.replace(/\\/g, '/').split('/').filter(Boolean)
  if (parts.length <= 1) return []
  const out: string[] = []
  let prefix = ''
  for (let i = 0; i < parts.length - 1; i++) {
    prefix = prefix ? `${prefix}/${parts[i]}` : parts[i]!
    out.push(prefix)
  }
  return out
}

export function findWorkspaceTreeNode(
  roots: WorkspaceTreeNodeModel[],
  path: string
): WorkspaceTreeNodeModel | null {
  const target = path.replace(/\\/g, '/')
  const stack = [...roots]
  while (stack.length) {
    const node = stack.pop()!
    if (node.path === target) return node
    if (node.children?.length) stack.push(...node.children)
  }
  return null
}
