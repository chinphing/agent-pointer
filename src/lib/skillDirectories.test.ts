import { describe, expect, it } from 'vitest'
import { userSkillDirectoryCandidates } from './skillDirectories'
import type { Project, SkillDef } from '../types/chat'

function skill(partial: Partial<SkillDef> & { id: string; name: string }): SkillDef {
  return {
    description: '',
    tags: [],
    toolNames: [],
    scenario: '',
    builtin: false,
    ...partial
  }
}

function project(partial: Partial<Project> & { id: string; workspaceRoot: string }): Project {
  return {
    name: partial.id,
    isDefault: false,
    isArchived: false,
    isPinned: false,
    lastActivityAt: 0,
    createdAt: 0,
    ...partial
  } as Project
}

describe('userSkillDirectoryCandidates', () => {
  it('includes user and external skills with a source path', () => {
    const skills = [
      skill({ id: 'cwpt-submit', name: 'cwpt-submit', provenance: 'user', source: '/Users/u/.pointer/skills/cwpt-submit' }),
      skill({ id: 'agent-browser', name: 'agent-browser', provenance: 'external', source: '/Users/u/.agents/skills/agent-browser' })
    ]
    expect(userSkillDirectoryCandidates(skills, [])).toEqual([
      { name: 'agent-browser', path: '/Users/u/.agents/skills/agent-browser' },
      { name: 'cwpt-submit', path: '/Users/u/.pointer/skills/cwpt-submit' }
    ])
  })

  it('excludes bundled system skills and skills without a source', () => {
    const skills = [
      skill({ id: 'pdf', name: 'pdf', provenance: 'system', source: '/app/PointerApp/skills/pdf' }),
      skill({ id: 'no-path', name: 'no-path', provenance: 'user' })
    ]
    expect(userSkillDirectoryCandidates(skills, [])).toEqual([])
  })

  it('excludes directories that are already project roots', () => {
    const skills = [
      skill({ id: 'docx', name: 'docx', provenance: 'user', source: '/Users/u/.pointer/skills/docx' })
    ]
    const projects = [
      project({ id: 'p1', workspaceRoot: '/Users/u/.pointer/skills/docx' }),
      project({ id: 'p2', workspaceRoot: '/Users/u/.pointer/skills/docx', isArchived: true })
    ]
    expect(userSkillDirectoryCandidates(skills, projects)).toEqual([])
  })

  it('dedupes same path and sorts by name', () => {
    const skills = [
      skill({ id: 'b', name: 'b', provenance: 'user', source: '/Users/u/.pointer/skills/shared' }),
      skill({ id: 'a', name: 'a', provenance: 'external', source: '/Users/u/.pointer/skills/shared' })
    ]
    expect(userSkillDirectoryCandidates(skills, [])).toEqual([
      { name: 'a', path: '/Users/u/.pointer/skills/shared' }
    ])
  })
})
