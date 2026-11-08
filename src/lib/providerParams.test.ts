import { describe, expect, it } from 'vitest'
import {
  detectProviderTemplateId,
  firstPresetTemplate,
  isDoubaoProvider,
  modelSupportsHostedWebSearch,
  preferredWebSearchTier,
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
    // 前两位决定默认模型与网页搜索档位，顺序不能变；后面的按目录追加。
    expect(byId.deepseek.defaultModels.slice(0, 2)).toEqual([
      'deepseek-flash',
      'deepseek-v4-pro'
    ])
    expect(byId.deepseek.defaultModels).toContain('deepseek-v4-flash')
    expect(byId.qwen.defaultBaseUrl).toBe('https://dashscope.aliyuncs.com/compatible-mode/v1')
    expect(byId.qwen.defaultModels.slice(0, 5)).toEqual([
      'qwen3.8-max',
      'qwen3.8-flash',
      'qwen3.8-27b',
      'qwen3.7-plus',
      'qwen3.7-flash'
    ])
    // 档位默认值引用的模型必须在预设里，否则新用户建的服务商调不通。
    for (const model of ['qwen3.5-flash', 'qwen3.5-plus', 'qwen3.7-max', 'fun-asr']) {
      expect(byId.qwen.defaultModels).toContain(model)
    }
    expect(byId.mimo.defaultBaseUrl).toBe('https://api.xiaomimimo.com/v1')
    expect(byId.mimo.defaultModels).toEqual([
      'mimo-v2.6-pro',
      'mimo-v2.6-flash',
      'mimo-v2.6-pro-ultraspeed'
    ])
    expect(byId.kimi.defaultBaseUrl).toBe('https://api.moonshot.cn/v1')
    expect(byId.kimi.defaultModels).toEqual([
      'kimi-k3',
      'kimi-k3.1-pro',
      'kimi-k2.7-code',
      'kimi-k2.7-code-highspeed'
    ])
    expect(byId.zhipu.label).toBe('智谱 GLM')
    expect(byId.zhipu.defaultBaseUrl).toBe('https://open.bigmodel.cn/api/paas/v4')
    expect(byId.zhipu.defaultModels).toEqual(['glm-5.3', 'glm-5.3-flash', 'glm-5.3-flashx', 'glm-5.2'])
    // 企业预置的三家排最前，其余按目录顺序
    expect(PROVIDER_TEMPLATE_OPTIONS.map(option => option.id).slice(0, 3)).toEqual([
      'qwen',
      'deepseek',
      'doubao'
    ])
  })

  it('offers Qwen and any DeepSeek model for hosted web search', () => {
    const qwen = { id: 'qwen', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1' }
    const deepseek = { id: 'deepseek', baseUrl: 'https://api.deepseek.com/v1' }
    expect(modelSupportsHostedWebSearch(qwen, 'qwen3.8-max')).toBe(true)
    expect(modelSupportsHostedWebSearch(deepseek, 'deepseek-v4-pro')).toBe(true)
    expect(modelSupportsHostedWebSearch(deepseek, 'deepseek-v4-pro-2026-08-01')).toBe(true)
    expect(modelSupportsHostedWebSearch(deepseek, 'deepseek-flash')).toBe(true)
    expect(modelSupportsHostedWebSearch(deepseek, '  ')).toBe(false)
    expect(modelSupportsHostedWebSearch({ id: 'kimi', baseUrl: 'https://api.moonshot.cn/v1' }, 'kimi-k3')).toBe(
      false
    )
  })

  it('prefers the first DeepSeek model for the web search tier', () => {
    expect(
      preferredWebSearchTier([
        { id: 'qwen', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', models: ['qwen3.8-max'] },
        { id: 'deepseek', baseUrl: 'https://api.deepseek.com/v1', models: ['deepseek-flash', 'deepseek-v4-pro'] }
      ])
    ).toEqual({ providerId: 'deepseek', model: 'deepseek-flash' })
    expect(
      preferredWebSearchTier([
        { id: 'qwen', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', models: ['qwen3.8-max'] }
      ])
    ).toBeUndefined()
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
      name: '小米 MiMo',
      baseUrl: 'https://api.xiaomimimo.com/v1',
      apiKey: '',
      models: ['mimo-v2.6-pro', 'mimo-v2.6-flash', 'mimo-v2.6-pro-ultraspeed']
    })
    // 已占用 qwen/deepseek/doubao 后，第一个可用的预设是 zhipu（按目录顺序）
    expect(firstPresetTemplate(new Set(['qwen', 'deepseek', 'doubao']))).toBe('zhipu')
    const allIds = new Set(PROVIDER_TEMPLATE_OPTIONS.map(o => o.defaultId).filter(Boolean))
    expect(firstPresetTemplate(allIds)).toBe('openai_compatible')
  })
})
