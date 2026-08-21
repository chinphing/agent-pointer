import { describe, expect, it } from 'vitest'
import type { ProviderConfig } from '../types/chat'
import {
  modelCanGenerateImage,
  modelCanGenerateVideo,
  modelSupportsVision,
  resolvedModelCapabilities
} from './modelCapabilities'

function provider(over: ProviderConfig['modelConfigs'] = {}): ProviderConfig {
  return {
    id: 'qwen',
    name: 'Qwen',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    apiKey: 'sk-test',
    models: ['qwen3.5-plus', 'wan2.7-image-pro', 'happyhorse-1.0-t2v'],
    modelConfigs: over
  }
}

describe('resolvedModelCapabilities', () => {
  it('treats unset flags as off even when the model name looks generative', () => {
    const p = provider()
    expect(modelSupportsVision([p], 'qwen', 'qwen3.5-plus')).toBe(false)
    expect(modelCanGenerateImage([p], 'qwen', 'wan2.7-image-pro')).toBe(false)
    expect(modelCanGenerateVideo([p], 'qwen', 'happyhorse-1.0-t2v')).toBe(false)
  })

  it('honors explicit catalog flags only', () => {
    const p = provider({
      'qwen3.5-plus': { supportsVision: true },
      'wan2.7-image-pro': { canGenerateImage: true },
      'happyhorse-1.0-t2v': { canGenerateVideo: true }
    })
    expect(resolvedModelCapabilities([p], 'qwen', 'qwen3.5-plus')).toEqual({
      supportsVision: true,
      supportsAudio: false,
      canGenerateImage: false,
      canGenerateVideo: false
    })
    expect(modelCanGenerateImage([p], 'qwen', 'wan2.7-image-pro')).toBe(true)
    expect(modelCanGenerateVideo([p], 'qwen', 'happyhorse-1.0-t2v')).toBe(true)
    expect(modelCanGenerateImage([p], 'qwen', 'qwen3.5-plus')).toBe(false)
  })
})
