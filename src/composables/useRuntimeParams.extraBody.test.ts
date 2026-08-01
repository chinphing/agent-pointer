import { describe, expect, it } from 'vitest'
import {
  formatExtraBodyJson,
  hasEffectiveModelOverride,
  normalizeExtraBody,
  parseExtraBodyJson,
  sanitizeProviderModelConfigs
} from './useRuntimeParams'
import type { ProviderConfig } from '../types/chat'

describe('extraBody helpers', () => {
  it('parses and formats Hermes-style objects', () => {
    expect(parseExtraBodyJson('')).toEqual({ ok: true, value: undefined })
    expect(parseExtraBodyJson('{ "repetition_penalty": 1.1 }')).toEqual({
      ok: true,
      value: { repetition_penalty: 1.1 }
    })
    expect(parseExtraBodyJson('[]').ok).toBe(false)
    expect(formatExtraBodyJson({ top_p: 0.8 })).toContain('top_p')
    expect(normalizeExtraBody({})).toBeUndefined()
  })

  it('keeps non-empty extraBody as effective model override', () => {
    const p: ProviderConfig = {
      id: 'local',
      name: 'local',
      baseUrl: 'http://localhost:8000/v1',
      apiKey: '',
      models: ['m1']
    }
    const fallback = { temperature: () => 0.7, maxTokens: () => 2048 }
    expect(
      hasEffectiveModelOverride({ extraBody: { repetition_penalty: 1.1 } }, p, fallback, 'm1')
    ).toBe(true)
    const sanitized = sanitizeProviderModelConfigs(
      p,
      ['m1'],
      { m1: { extraBody: { repetition_penalty: 1.1 } } },
      fallback
    )
    expect(sanitized.m1?.extraBody).toEqual({ repetition_penalty: 1.1 })
  })
})
