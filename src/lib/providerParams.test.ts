import { describe, expect, it } from 'vitest'
import { detectProviderTemplateId, isDoubaoProvider } from './providerParams'

describe('platform provider detection', () => {
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

  it('falls back to openai_compatible for unknown providers', () => {
    expect(
      detectProviderTemplateId({ id: 'custom-llm', baseUrl: 'https://custom.example/v1' })
    ).toBe('openai_compatible')
  })
})
