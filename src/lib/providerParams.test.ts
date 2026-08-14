import { describe, expect, it } from 'vitest'
import {
  detectProviderTemplateId,
  isDoubaoProvider,
  providerDraftForTemplate
} from './providerParams'

const globalDefaults = { temperature: 0.7, maxTokens: 2048 }

describe('platform provider templates', () => {
  it('recognizes doubao id and Ark endpoints as the platform doubao template', () => {
    expect(detectProviderTemplateId({ id: 'doubao', baseUrl: '' })).toBe('doubao')
    expect(
      detectProviderTemplateId({
        id: 'volcengine-prod',
        baseUrl: 'https://ark.cn-beijing.volces.com/api/v3'
      })
    ).toBe('doubao')
    expect(isDoubaoProvider({ id: 'custom', baseUrl: 'https://api.example.com/v1' })).toBe(false)
  })

  it('creates a valid doubao draft', () => {
    const draft = providerDraftForTemplate('doubao', globalDefaults)
    expect(draft).toMatchObject({
      id: 'doubao',
      name: '豆包',
      baseUrl: 'https://ark.cn-beijing.volces.com/api/v3'
    })
    expect(draft.models.length).toBeGreaterThan(0)
  })
})
