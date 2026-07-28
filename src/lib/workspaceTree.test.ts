import { describe, expect, it } from 'vitest'
import {
  findWorkspaceTreeNode,
  mergeWorkspaceTreePreserveState,
  workspacePathAncestorDirs,
  type WorkspaceTreeNodeModel
} from './workspaceTree'

describe('workspaceTree helpers', () => {
  it('preserves expanded children across silent root refresh', () => {
    const previous: WorkspaceTreeNodeModel[] = [
      {
        name: 'src',
        path: 'src',
        kind: 'directory',
        expanded: true,
        children: [{ name: 'App.vue', path: 'src/App.vue', kind: 'file', sizeBytes: 10 }]
      },
      { name: 'README.md', path: 'README.md', kind: 'file', sizeBytes: 4 }
    ]
    const next = [
      { name: 'README.md', path: 'README.md', kind: 'file' as const, sizeBytes: 4 },
      { name: 'src', path: 'src', kind: 'directory' as const }
    ]
    const merged = mergeWorkspaceTreePreserveState(previous, next)
    const src = merged.find(n => n.path === 'src')
    expect(src?.expanded).toBe(true)
    expect(src?.children?.[0]?.path).toBe('src/App.vue')
  })

  it('lists ancestor dirs and finds nested nodes', () => {
    expect(workspacePathAncestorDirs('src/components/A.vue')).toEqual([
      'src',
      'src/components'
    ])
    const roots: WorkspaceTreeNodeModel[] = [
      {
        name: 'src',
        path: 'src',
        kind: 'directory',
        expanded: true,
        children: [{ name: 'A.vue', path: 'src/A.vue', kind: 'file' }]
      }
    ]
    expect(findWorkspaceTreeNode(roots, 'src/A.vue')?.name).toBe('A.vue')
  })
})
