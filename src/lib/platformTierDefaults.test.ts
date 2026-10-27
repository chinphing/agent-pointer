import { describe, expect, it } from 'vitest'
import {
  isSameTierRef,
  normalizePlatformProviderTemplates,
  platformAgentModeDefault,
  platformComputerPipelineDefault,
  platformMediaGenerationDefault,
  platformModelNames,
  withInheritedThinking
} from './platformTierDefaults'

describe('platformTierDefaults', () => {
  it('extracts model names from string and object entries', () => {
    expect(
      platformModelNames([
        'qwen-next',
        { name: 'qwen-next' },
        { name: 'qwen3.8-max', maxTokens: 8192 },
        ' ',
        12
      ])
    ).toEqual(['qwen-next', 'qwen3.8-max'])
  })

  it('ignores platform thinkingProtocol and keeps intensity', () => {
    const providers = normalizePlatformProviderTemplates([
      {
        id: 'qwen',
        name: '千问',
        baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
        thinkingProtocol: 'budget',
        thinkingIntensity: 'medium',
        models: [{ name: 'qwen3.5-plus', thinkingProtocol: 'budget', thinkingIntensity: 'high' }]
      }
    ])
    expect(providers[0].thinkingProtocol).toBeUndefined()
    expect(providers[0].thinkingIntensity).toBe('medium')
    expect(providers[0].modelConfigs?.['qwen3.5-plus']).toEqual({ thinkingIntensity: 'high' })
  })

  it('builds providers from an API array of templates', () => {
    const providers = normalizePlatformProviderTemplates([
      {
        id: 'qwen',
        name: '千问',
        baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
        models: [
          { name: 'qwen-next', maxTokens: 4096, enableThinking: true },
          'qwen3.8-max'
        ]
      }
    ])
    expect(providers).toHaveLength(1)
    expect(providers[0].id).toBe('qwen')
    expect(providers[0].models).toEqual(['qwen-next', 'qwen3.8-max'])
    expect(providers[0].modelConfigs?.['qwen-next']).toEqual({
      maxTokens: 4096,
      enableThinking: true
    })
  })

  it('copies context budget from platform provider and model entries', () => {
    const providers = normalizePlatformProviderTemplates([
      {
        id: 'qwen',
        name: '千问',
        baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
        contextBudgetTokens: 128_000,
        models: [
          { name: 'qwen-next', contextBudgetTokens: 64_000 },
          'qwen3.8-max'
        ]
      }
    ])
    expect(providers[0].contextBudgetTokens).toBe(128_000)
    expect(providers[0].modelConfigs?.['qwen-next']?.contextBudgetTokens).toBe(64_000)
    expect(providers[0].modelConfigs?.['qwen3.8-max']).toBeUndefined()
  })

  it('reads scene defaults from platform tierDefaults', () => {
    const tierDefaults = {
      agentModeLlm: {
        general: { fast: { providerId: 'qwen', model: 'qwen-next' } }
      },
      computerPipelineLlm: {
        verify: { providerId: 'qwen', model: 'qwen3.8-max' }
      },
      mediaGeneration: {
        image: { providerId: 'doubao', model: 'doubao-seedream-5-0-lite-260128' },
        video: { providerId: 'doubao', model: 'doubao-seedance-2-0-fast-260128' }
      }
    }
    expect(platformAgentModeDefault(tierDefaults, 'general', 'fast')).toEqual({
      providerId: 'qwen',
      model: 'qwen-next'
    })
    expect(platformMediaGenerationDefault(tierDefaults, 'image')).toEqual({
      providerId: 'doubao',
      model: 'doubao-seedream-5-0-lite-260128'
    })
    expect(platformMediaGenerationDefault(tierDefaults, 'video')).toEqual({
      providerId: 'doubao',
      model: 'doubao-seedance-2-0-fast-260128'
    })
    expect(platformComputerPipelineDefault(tierDefaults)).toEqual({
      verify: 'qwen3.8-max',
      verifyProviderId: 'qwen'
    })
    expect(isSameTierRef({ providerId: 'qwen', model: 'qwen-next' }, {
      providerId: 'qwen',
      model: 'qwen-next'
    })).toBe(true)
  })

  it('inherits thinking flags from platform model configs', () => {    const cfg = withInheritedThinking(
      { providerId: 'qwen', model: 'qwen-next' },
      [
        {
          id: 'qwen',
          name: '千问',
          baseUrl: 'https://example',
          apiKey: '',
          models: ['qwen-next'],
          enableThinking: true,
          thinkingBudget: 2048,
          modelConfigs: { 'qwen-next': { enableThinking: true, thinkingBudget: 8192 } }
        }
      ]
    )
    expect(cfg).toEqual({
      providerId: 'qwen',
      model: 'qwen-next',
      thinkingIntensity: 'max',
      enableThinking: true,
      thinkingBudget: 8192,
      reasoningEffort: 'max'
    })
  })

  it('uses tier thinking intensity dispatched by the platform', () => {
    const ref = platformAgentModeDefault(
      {
        agentModeLlm: {
          general: {
            fast: {
              providerId: 'qwen',
              model: 'qwen-next',
              thinkingIntensity: 'max'
            }
          }
        }
      },
      'general',
      'fast'
    )
    expect(ref).toEqual({ providerId: 'qwen', model: 'qwen-next', thinkingIntensity: 'max' })
    const cfg = withInheritedThinking(ref!, [
      {
        id: 'qwen',
        name: '千问',
        baseUrl: 'https://example',
        apiKey: '',
        models: ['qwen-next'],
        modelConfigs: { 'qwen-next': { thinkingIntensity: 'low' } }
      }
    ])
    // 档位下发值覆盖模型级继承。
    expect(cfg.thinkingIntensity).toBe('max')
    expect(cfg.thinkingBudget).toBe(8192)
  })

  it('keeps inheriting model thinking when the tier omits intensity', () => {
    const ref = platformAgentModeDefault(
      { agentModeLlm: { general: { fast: { providerId: 'qwen', model: 'qwen-next' } } } },
      'general',
      'fast'
    )
    expect(ref).toEqual({ providerId: 'qwen', model: 'qwen-next' })
    const cfg = withInheritedThinking(ref!, [
      {
        id: 'qwen',
        name: '千问',
        baseUrl: 'https://example',
        apiKey: '',
        models: ['qwen-next'],
        thinkingIntensity: 'high'
      }
    ])
    expect(cfg.thinkingIntensity).toBe('high')
  })

  it('reads pipeline locate and thinking intensity from platform tierDefaults', () => {
    const out = platformComputerPipelineDefault({
      computerPipelineLlm: {
        verify: {
          providerId: 'qwen',
          model: 'qwen3.5-flash',
          thinkingIntensity: 'high'
        },
        locate: { providerId: '', model: '', thinkingIntensity: 'off' }
      }
    })
    expect(out.verify).toBe('qwen3.5-flash')
    expect(out.verifyProviderId).toBe('qwen')
    expect(out.verifyThinkingBudget).toBe(4096)
    // 定位模型留空 = 跟随主模型，思考强度仍独立生效。
    expect(out.locate).toBeUndefined()
    expect(out.locateThinkingBudget).toBe(0)
  })

  it('reads a platform locate model when one is set', () => {
    const out = platformComputerPipelineDefault({
      computerPipelineLlm: {
        locate: {
          providerId: 'qwen',
          model: 'qwen3.5-plus',
          thinkingIntensity: 'low'
        }
      }
    })
    expect(out.locate).toBe('qwen3.5-plus')
    expect(out.locateProviderId).toBe('qwen')
    expect(out.locateThinkingBudget).toBe(1024)
  })

  it('reads locate verify model and thinking from platform tierDefaults', () => {
    const out = platformComputerPipelineDefault({
      computerPipelineLlm: {
        locateVerify: {
          providerId: 'qwen',
          model: 'qwen3.5-plus',
          thinkingIntensity: 'max'
        }
      }
    })
    expect(out.locateVerify).toBe('qwen3.5-plus')
    expect(out.locateVerifyProviderId).toBe('qwen')
    expect(out.locateVerifyThinkingBudget).toBe(8192)
  })

  it('keeps locate verify following locate when the platform omits it', () => {
    const out = platformComputerPipelineDefault({
      computerPipelineLlm: { locate: { providerId: 'qwen', model: 'qwen3.5-flash' } }
    })
    expect(out.locateVerify).toBeUndefined()
    expect(out.locateVerifyThinkingBudget).toBeUndefined()
  })
})
