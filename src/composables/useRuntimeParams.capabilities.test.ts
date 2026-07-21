import { describe, expect, it } from 'vitest'
import type { ProviderConfig } from '../types/chat'
import {
  hasEffectiveModelOverride,
  patchProviderModelCapability,
  pruneInheritedModelConfigs,
  sanitizeProviderModelConfigs
} from './useRuntimeParams'

const fallback = {
  temperature: () => 0.7,
  maxTokens: () => 2048
}

function qwenProvider(over: ProviderConfig['modelConfigs'] = {}): ProviderConfig {
  return {
    id: 'qwen',
    name: 'Qwen',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    apiKey: 'sk-test',
    models: ['qwen3.5-plus', 'wan2.7-image-pro'],
    modelConfigs: over,
    temperature: 0.7,
    maxTokens: 2048
  }
}

describe('model capability overrides', () => {
  it('treats vision overrides that differ from provider defaults as effective', () => {
    expect(
      hasEffectiveModelOverride(
        { supportsVision: false },
        qwenProvider(),
        fallback,
        'qwen3.5-plus'
      )
    ).toBe(true)

    expect(
      hasEffectiveModelOverride(
        { supportsVision: true },
        qwenProvider(),
        fallback,
        'qwen3.5-plus'
      )
    ).toBe(false)
  })

  it('treats generation capability overrides as effective', () => {
    expect(
      hasEffectiveModelOverride(
        { canGenerateImage: true },
        qwenProvider(),
        fallback,
        'qwen3.5-plus'
      )
    ).toBe(true)

    expect(
      hasEffectiveModelOverride(
        { canGenerateImage: true },
        qwenProvider(),
        fallback,
        'wan2.7-image-pro'
      )
    ).toBe(false)

    expect(
      hasEffectiveModelOverride(
        { canGenerateVideo: true },
        qwenProvider(),
        fallback,
        'qwen3.5-plus'
      )
    ).toBe(true)
  })

  it('keeps capability-only custom entries when pruning inherited configs', () => {
    const pruned = pruneInheritedModelConfigs(
      qwenProvider({
        'qwen3.5-plus': { supportsVision: false },
        'wan2.7-image-pro': { canGenerateImage: true, temperature: 0.7 }
      }),
      {
        'qwen3.5-plus': { supportsVision: false },
        'wan2.7-image-pro': { canGenerateImage: true, temperature: 0.7 }
      },
      fallback
    )

    expect(pruned['qwen3.5-plus']).toEqual({ supportsVision: false })
    expect(pruned['wan2.7-image-pro']).toBeUndefined()
  })

  it('keeps capability patches through provider save sanitization', () => {
    const draft = patchProviderModelCapability(
      qwenProvider({
        'qwen3.5-plus': {
          temperature: 0.7,
          maxTokens: 2048,
          reasoningInMessages: false,
          enableThinking: false
        }
      }),
      'qwen3.5-plus',
      'supportsVision',
      false
    )

    expect(draft.modelConfigs?.['qwen3.5-plus']?.supportsVision).toBe(false)

    const sanitized = sanitizeProviderModelConfigs(
      draft,
      draft.models,
      draft.modelConfigs,
      fallback
    )

    expect(sanitized['qwen3.5-plus']?.supportsVision).toBe(false)
    expect(sanitized['qwen3.5-plus']?.enableThinking).toBe(false)
  })
})
