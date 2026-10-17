import { describe, expect, it } from 'vitest'
import {
  detectProviderTemplateId,
  firstPresetTemplate,
  isDoubaoProvider,
  PROVIDER_TEMPLATE_OPTIONS,
  providerDraftForTemplate
} from './providerParams'

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

  it('presets DeepSeek, Qwen, MiMo, Kimi, and GLM with a base URL and models', () => {
    const byId = Object.fromEntries(PROVIDER_TEMPLATE_OPTIONS.map(option => [option.id, option]))
    expect(byId.deepseek.defaultBaseUrl).toBe('https://api.deepseek.com/v1')
    expect(byId.deepseek.defaultModels).toEqual(['deepseek-v4-pro', 'deepseek-flash'])
    expect(byId.qwen.defaultBaseUrl).toBe('https://dashscope.aliyuncs.com/compatible-mode/v1')
    expect(byId.qwen.defaultModels).toEqual([
      'qwen3.8-max',
      'qwen3.8-omni-flash',
      'qwen3.8-flash',
      'qwen3.8-27b'
    ])
    expect(byId.mimo.defaultBaseUrl).toBe('https://api.xiaomimimo.com/v1')
    expect(byId.mimo.defaultModels).toEqual([
      'mimo-v2.6-pro',
      'mimo-v2.6-flash',
      'mimo-v2.6-pro-ultraspeed'
    ])
    expect(byId.kimi.defaultBaseUrl).toBe('https://api.moonshot.cn/v1')
    expect(byId.kimi.defaultModels).toEqual([
      'kimi-k3',
      'kimi-k2.7-code',
      'kimi-k2.7-code-highspeed',
      'kimi-k2.6'
    ])
    expect(byId.zhipu.label).toBe('GLM')
    expect(byId.zhipu.defaultBaseUrl).toBe('https://open.bigmodel.cn/api/paas/v4')
    expect(byId.zhipu.defaultModels).toEqual(['glm-5.3', 'glm-5.3-flash', 'glm-5.3-flashx', 'glm-5.2'])
    expect(PROVIDER_TEMPLATE_OPTIONS.map(option => option.id).slice(0, 5)).toEqual([
      'deepseek',
      'qwen',
      'mimo',
      'kimi',
      'zhipu'
    ])
  })

  it('detects MiMo, GLM, and Kimi from id or host', () => {
    expect(detectProviderTemplateId({ id: 'mimo', baseUrl: '' })).toBe('mimo')
    expect(
      detectProviderTemplateId({ id: 'custom', baseUrl: 'https://token-plan-cn.xiaomimimo.com/v1' })
    ).toBe('mimo')
    expect(detectProviderTemplateId({ id: 'glm', baseUrl: '' })).toBe('zhipu')
    expect(detectProviderTemplateId({ id: 'custom', baseUrl: 'https://api.moonshot.ai/v1' })).toBe('kimi')
  })

  it('fills a draft from the preset and skips occupied ids', () => {
    const draft = providerDraftForTemplate('mimo', {
      temperature: 0.3,
      maxTokens: 64000
    })
    expect(draft).toMatchObject({
      id: 'mimo',
      name: 'MiMo',
      baseUrl: 'https://api.xiaomimimo.com/v1',
      apiKey: '',
      models: ['mimo-v2.6-pro', 'mimo-v2.6-flash', 'mimo-v2.6-pro-ultraspeed']
    })
    expect(firstPresetTemplate(new Set(['deepseek', 'qwen']))).toBe('mimo')
    expect(
      firstPresetTemplate(new Set(['deepseek', 'qwen', 'mimo', 'kimi', 'zhipu', 'openrouter']))
    ).toBe('openai_compatible')
  })
})
