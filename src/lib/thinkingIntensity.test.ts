import { describe, expect, it } from 'vitest'
import {
  mergeTierLlmPatch,
  thinkingPatchFromProviderModel
} from './thinkingIntensity'
import type { ComputerTierLlmConfig } from '../types/chat'

describe('thinkingPatchFromProviderModel', () => {
  const providers = [
    {
      id: 'qwen',
      thinkingIntensity: 'medium' as const,
      enableThinking: true,
      thinkingBudget: 2048,
      modelConfigs: {
        'qwen-plus': { thinkingIntensity: 'high' as const, enableThinking: true, thinkingBudget: 4096 }
      }
    }
  ]

  it('stamps the model catalog intensity when selecting a model', () => {
    expect(thinkingPatchFromProviderModel(providers, 'qwen', 'qwen-plus')).toEqual({
      thinkingIntensity: 'high',
      enableThinking: true,
      thinkingBudget: 4096,
      reasoningEffort: 'high'
    })
  })

  it('falls back to provider intensity when the model has none', () => {
    expect(thinkingPatchFromProviderModel(providers, 'qwen', 'unknown')).toEqual({
      thinkingIntensity: 'medium',
      enableThinking: true,
      thinkingBudget: 2048,
      reasoningEffort: undefined
    })
  })

  it('clears thinking when the catalog has no default', () => {
    expect(thinkingPatchFromProviderModel([{ id: 'local' }], 'local', 'm')).toEqual({
      thinkingIntensity: undefined,
      enableThinking: undefined,
      thinkingBudget: undefined,
      reasoningEffort: undefined
    })
  })
})

describe('mergeTierLlmPatch', () => {
  it('drops prior intensity when the user picks 不设置', () => {
    const prev: ComputerTierLlmConfig = {
      providerId: 'qwen',
      model: 'qwen-plus',
      thinkingIntensity: 'high',
      enableThinking: true,
      thinkingBudget: 4096
    }
    expect(
      mergeTierLlmPatch(prev, {
        thinkingIntensity: undefined,
        enableThinking: undefined,
        thinkingBudget: undefined,
        reasoningEffort: undefined
      })
    ).toEqual({ providerId: 'qwen', model: 'qwen-plus' })
  })
})
