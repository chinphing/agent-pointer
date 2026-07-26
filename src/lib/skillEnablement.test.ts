import { describe, expect, it } from 'vitest'
import { missingDefaultSystemSkillIds, missingSystemSkillIds } from './skillEnablement'

describe('skillEnablement', () => {
  it('finds system skills missing from general', () => {
    expect(
      missingSystemSkillIds(
        ['skill-manager', 'docx', 'pdf'],
        ['docx', 'pdf', 'xlsx']
      )
    ).toEqual(['skill-manager'])
  })

  it('finds coder default system skills missing from a stale override', () => {
    const system = new Set([
      'skill-manager',
      'find-skills',
      'docx',
      'xlsx',
      'pptx',
      'pdf',
      'agent-browser',
      'dev-env-setup',
      'pointer-manager'
    ])
    const coderDefaults = [
      'find-skills',
      'skill-manager',
      'xlsx',
      'pdf',
      'agent-browser',
      'dev-env-setup',
      'docx',
      'pptx'
    ]
    const staleOverride = [
      'agent-browser',
      'dev-env-setup',
      'docx',
      'find-skills',
      'pdf',
      'pptx',
      'xlsx'
    ]
    expect(missingDefaultSystemSkillIds(coderDefaults, system, staleOverride)).toEqual([
      'skill-manager'
    ])
  })
})
