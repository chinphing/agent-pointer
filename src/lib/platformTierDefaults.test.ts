import { describe, expect, it } from 'vitest'
import {
  isSameTierRef,
  normalizePlatformProviderTemplates,
  platformAgentModeDefault,
  platformComputerPipelineDefault,
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

  it('reads scene defaults from platform tierDefaults', () => {
    const tierDefaults = {
      agentModeLlm: {
        general: { fast: { providerId: 'qwen', model: 'qwen-next' } }
      },
      computerPipelineLlm: {
        verify: { providerId: 'qwen', model: 'qwen3.8-max' }
      }
    }
    expect(platformAgentModeDefault(tierDefaults, 'general', 'fast')).toEqual({
      providerId: 'qwen',
      model: 'qwen-next'
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

  it('inherits thinking flags from platform model configs', () => {
    const cfg = withInheritedThinking(
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
      enableThinking: true,
      thinkingBudget: 8192
    })
  })
})
