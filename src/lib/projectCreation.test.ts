import { readFileSync } from 'node:fs'
import { describe, expect, it, vi } from 'vitest'
import { applyProjectCreationResult, projectNameFromWorkspaceRoot } from './projectCreation'
import type { Project, ProjectCreationResult } from '../types/chat'

const appShellSource = readFileSync(new URL('../components/layout/AppShell.vue', import.meta.url), 'utf8')
const composerSource = readFileSync(new URL('../components/chat/Composer.vue', import.meta.url), 'utf8')

const project: Project = {
  id: 'project-existing',
  name: 'Existing project',
  workspaceRoot: '/workspace/existing',
  isDefault: false,
  isPinned: false,
  isArchived: false,
  createdAt: 1,
  updatedAt: 1,
  lastActivityAt: 1
}

function result(reusedExisting: boolean): ProjectCreationResult {
  return { project, reusedExisting }
}

describe('project creation dialog submission', () => {
  it('uses native form submission so Enter in either input creates the project', () => {
    expect(appShellSource).toContain('<form class="mt-4" @submit.prevent="addProject">')
    expect(appShellSource).toContain('<button type="submit" class="project-dialog-primary"')
    expect(appShellSource).not.toContain('@keydown.enter="addProject"')
  })

  it('creates or reuses a project when the composer chooses a workspace directory', () => {
    expect(composerSource).toContain('await createOrSelectWorkspaceProject(dir)')
    expect(composerSource).toContain('@keydown.enter.prevent="commitWorkspaceInput"')
  })
})

describe('projectNameFromWorkspaceRoot', () => {
  it.each([
    ['/workspace/pointer-app/', 'pointer-app'],
    ['C:\\work\\pointer-app\\', 'pointer-app'],
    ['  /workspace/pointer-app  ', 'pointer-app']
  ])('derives a cross-platform project name from %s', (workspaceRoot, expected) => {
    expect(projectNameFromWorkspaceRoot(workspaceRoot)).toBe(expected)
  })
})

describe('applyProjectCreationResult', () => {
  it('refreshes, selects, and names an existing project for a duplicate workspace', async () => {
    const refreshProjects = vi.fn().mockResolvedValue(undefined)
    const selectProject = vi.fn().mockResolvedValue(undefined)
    const notify = vi.fn()

    await applyProjectCreationResult(result(true), { refreshProjects, selectProject, notify })

    expect(refreshProjects).toHaveBeenCalledOnce()
    expect(selectProject).toHaveBeenCalledWith(project.id)
    expect(notify).toHaveBeenCalledWith('目录已关联到现有项目「Existing project」')
    expect(refreshProjects.mock.invocationCallOrder[0]).toBeLessThan(selectProject.mock.invocationCallOrder[0]!)
  })

  it('refreshes and selects a newly created project immediately', async () => {
    const refreshProjects = vi.fn().mockResolvedValue(undefined)
    const selectProject = vi.fn().mockResolvedValue(undefined)
    const notify = vi.fn()

    await applyProjectCreationResult(result(false), { refreshProjects, selectProject, notify })

    expect(refreshProjects).toHaveBeenCalledOnce()
    expect(selectProject).toHaveBeenCalledWith(project.id)
    expect(notify).not.toHaveBeenCalled()
    expect(refreshProjects.mock.invocationCallOrder[0]).toBeLessThan(selectProject.mock.invocationCallOrder[0]!)
  })
})
