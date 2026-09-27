import { beforeAll, describe, expect, it } from 'vitest'
import { i18n } from '../i18n'
import { composerAgentLabel } from './agentUi'
import type { AgentDef } from '../types/chat'

beforeAll(() => {
  i18n.global.locale.value = 'en'
})

function agent(partial: Partial<AgentDef> & Pick<AgentDef, 'id'>): AgentDef {
  return {
    name: partial.id,
    profile: 'general',
    ui: { composerLabel: '通用助手' },
    ...partial
  } as AgentDef
}

describe('composerAgentLabel locale', () => {
  it('uses i18n for built-in general even when AGENT.md label is Chinese', () => {
    expect(composerAgentLabel(agent({ id: 'general' }))).toBe('General')
  })

  it('uses i18n for coder / computer', () => {
    expect(composerAgentLabel(agent({ id: 'coder', ui: { composerLabel: '氛围编程' } }))).toBe(
      'Vibe coding'
    )
    expect(composerAgentLabel(agent({ id: 'computer', ui: { composerLabel: '电脑操控' } }))).toBe(
      'Computer use'
    )
  })

  it('honors explicit user override', () => {
    expect(
      composerAgentLabel(agent({ id: 'general' }), {
        agentUiOverrides: { general: { composerLabel: 'My Helper' } }
      } as never)
    ).toBe('My Helper')
  })
})
