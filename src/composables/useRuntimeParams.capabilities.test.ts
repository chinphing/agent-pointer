import { describe, expect, it } from 'vitest'
import type { ProviderConfig } from '../types/chat'
import {
  hasEffectiveModelOverride,
  patchProviderModelCapability,
  pruneInheritedModelConfigs,
  sanitizeProviderModelConfigs
} from './useRuntimeParams'
import { modelSupportsAudioTranscription } from '../lib/modelCapabilities'

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
    it('keeps explicit vision flags even when they match the DashScope default', () => {
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
    ).toBe(true)
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
    ).toBe(true)

    expect(
      hasEffectiveModelOverride(
        { canGenerateVideo: true },
        qwenProvider(),
        fallback,
        'qwen3.5-plus'
      )
    ).toBe(true)
  })

  it('keeps catalog audio flags when pruning', () => {
    const pruned = pruneInheritedModelConfigs(
      qwenProvider({
        'qwen3-asr-flash': { supportsAudio: true }
      }),
      { 'qwen3-asr-flash': { supportsAudio: true } },
      fallback
    )
    expect(pruned['qwen3-asr-flash']).toEqual({ supportsAudio: true })
    expect(
      hasEffectiveModelOverride(
        { supportsAudio: false },
        qwenProvider(),
        fallback,
        'qwen3.5-plus'
      )
    ).toBe(true)
  })

  it('keeps catalog vision true when pruning DashScope models', () => {
    const pruned = pruneInheritedModelConfigs(
      qwenProvider({
        'qwen3.5-plus': { supportsVision: true }
      }),
      { 'qwen3.5-plus': { supportsVision: true } },
      fallback
    )
    expect(pruned['qwen3.5-plus']).toEqual({ supportsVision: true })
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
    expect(pruned['wan2.7-image-pro']).toEqual({
      canGenerateImage: true,
      temperature: 0.7
    })
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

describe('audio transcription picker', () => {
  it('lists only models with explicit supportsAudio', () => {
    const providers: ProviderConfig[] = [
      qwenProvider({
        'qwen3.5-plus': { supportsVision: true },
        'qwen3-asr-flash': { supportsAudio: true }
      })
    ]
    providers[0].models = ['qwen3.5-plus', 'qwen3-asr-flash']
    expect(modelSupportsAudioTranscription(providers, 'qwen', 'qwen3-asr-flash')).toBe(true)
    expect(modelSupportsAudioTranscription(providers, 'qwen', 'qwen3.5-plus')).toBe(false)
  })
})
