import { describe, expect, it } from 'vitest'
import { splitProviderModelValue } from '../lib/modelSelectValue'

describe('splitProviderModelValue', () => {
  it('parses a normal providerId:model value', () => {
    expect(splitProviderModelValue('qwen:qwen3.7-plus')).toEqual({
      providerId: 'qwen',
      model: 'qwen3.7-plus'
    })
    expect(splitProviderModelValue('deepseek:deepseek-v4-flash')).toEqual({
      providerId: 'deepseek',
      model: 'deepseek-v4-flash'
    })
  })

  it('disambiguates same-named models across providers', () => {
    // Two providers ship a model with the same id; the prefix must select the right one.
    expect(splitProviderModelValue('providerA:gpt-4o')).toEqual({
      providerId: 'providerA',
      model: 'gpt-4o'
    })
    expect(splitProviderModelValue('providerB:gpt-4o')).toEqual({
      providerId: 'providerB',
      model: 'gpt-4o'
    })
  })

  it('trims whitespace around both halves', () => {
    expect(splitProviderModelValue('  qwen  :  qwen3.5-plus  ')).toEqual({
      providerId: 'qwen',
      model: 'qwen3.5-plus'
    })
  })

  it('rejects malformed values', () => {
    expect(splitProviderModelValue('')).toBeNull()
    expect(splitProviderModelValue('qwen')).toBeNull()
    expect(splitProviderModelValue(':qwen3.5-plus')).toBeNull()
    expect(splitProviderModelValue('qwen:')).toBeNull()
    expect(splitProviderModelValue(':')).toBeNull()
    expect(splitProviderModelValue('   :   ')).toBeNull()
  })
})
