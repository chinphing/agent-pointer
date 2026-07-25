import { readFileSync } from 'node:fs'
import { describe, expect, it, vi } from 'vitest'
import { applyProjectCreationResult } from './projectCreation'
import type { Project, ProjectCreationResult } from '../types/chat'

const appShellSource = readFileSync(new URL('../components/layout/AppShell.vue', import.meta.url), 'utf8')

const project: Project = {
  id: 'project-existing',
  name: 'Existing project',
  workspaceRoot: '/workspace/existing',
  isDefault: false,
  isPinned: false,
  isArchived: false,
  createdAt: 1,
  updatedAt: 1
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
})

describe('applyProjectCreationResult', () => {
  it('refreshes, selects, and names an existing project for a duplicate workspace', async () => {
    const refreshProjects = vi.fn().mockResolvedValue(undefined)
    const switchProject = vi.fn().mockResolvedValue(undefined)
    const notify = vi.fn()

    await applyProjectCreationResult(result(true), { refreshProjects, switchProject, notify })

    expect(refreshProjects).toHaveBeenCalledOnce()
    expect(switchProject).toHaveBeenCalledWith(project.id)
    expect(notify).toHaveBeenCalledWith('目录已关联到现有项目「Existing project」')
    expect(refreshProjects.mock.invocationCallOrder[0]).toBeLessThan(switchProject.mock.invocationCallOrder[0]!)
  })

  it('refreshes and selects a newly created project immediately', async () => {
    const refreshProjects = vi.fn().mockResolvedValue(undefined)
    const switchProject = vi.fn().mockResolvedValue(undefined)
    const notify = vi.fn()

    await applyProjectCreationResult(result(false), { refreshProjects, switchProject, notify })

    expect(refreshProjects).toHaveBeenCalledOnce()
    expect(switchProject).toHaveBeenCalledWith(project.id)
    expect(notify).not.toHaveBeenCalled()
    expect(refreshProjects.mock.invocationCallOrder[0]).toBeLessThan(switchProject.mock.invocationCallOrder[0]!)
  })
})
