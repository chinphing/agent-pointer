import { describe, expect, it } from 'vitest'
import { buildModelPayload, buildProviderDefaults, payloadToState } from './modelPayload'

/**
 * 这些用例挡的是「界面上看着对、下发后不生效」这类错：
 * 字段名写错、枚举值客户端不认、留空写成 null。
 */
describe('model payload', () => {
  it('留空的字段不下发', () => {
    const out = buildModelPayload({ caps: {}, limits: {}, thinking: {} })
    expect(out).toEqual({})
  })

  it('三态布尔：是/否 写真布尔，跟随不写', () => {
    const out = buildModelPayload({
      caps: { supportsVision: 'true', supportsAudio: 'false', canGenerateImage: '' },
      limits: {},
      thinking: {},
    })
    expect(out).toEqual({ supportsVision: true, supportsAudio: false })
    expect('canGenerateImage' in out).toBe(false)
  })

  it('数字字段转数字，非法值丢弃', () => {
    const out = buildModelPayload({
      caps: {},
      limits: { contextBudgetTokens: '131072', maxTokens: 'abc', temperature: '0.7' },
      thinking: {},
    })
    expect(out.contextBudgetTokens).toBe(131072)
    expect(out.temperature).toBe(0.7)
    expect('maxTokens' in out).toBe(false)
  })

  it('reasoningEffort 只放行 high / max', () => {
    // 客户端 normalize_reasoning_effort 只认这两个，别的值会被静默丢掉
    expect(buildModelPayload({ caps: {}, limits: {}, thinking: { reasoningEffort: 'high' } })).toEqual({
      reasoningEffort: 'high',
    })
    expect(buildModelPayload({ caps: {}, limits: {}, thinking: { reasoningEffort: 'max' } })).toEqual({
      reasoningEffort: 'max',
    })
    expect(buildModelPayload({ caps: {}, limits: {}, thinking: { reasoningEffort: 'low' } })).toEqual({})
  })

  it('不产出 thinkingProtocol —— 客户端硬丢弃这个字段', () => {
    const out = buildModelPayload({
      caps: {},
      limits: {},
      thinking: { thinkingProtocol: 'x', thinkingIntensity: 'high' },
    })
    expect('thinkingProtocol' in out).toBe(false)
    expect(out.thinkingIntensity).toBe('high')
  })

  it('读回来能原样还原', () => {
    const payload = {
      supportsVision: true,
      contextBudgetTokens: 131072,
      maxTokens: 8192,
      thinkingIntensity: 'high',
      reasoningEffort: 'max',
      reasoningInMessages: false,
    }
    const state = payloadToState(payload)
    expect(state.caps.supportsVision).toBe('true')
    expect(state.caps.supportsAudio).toBe('')
    expect(state.limits.contextBudgetTokens).toBe('131072')
    expect(state.thinking.reasoningEffort).toBe('max')
    expect(state.thinking.reasoningInMessages).toBe('false')
    expect(buildModelPayload(state)).toEqual(payload)
  })

  it('payload 为空时全部是「跟随」', () => {
    const state = payloadToState({})
    expect(Object.values(state.caps).every((v) => v === '')).toBe(true)
    expect(Object.values(state.limits).every((v) => v === '')).toBe(true)
    expect(Object.values(state.thinking).every((v) => v === '')).toBe(true)
  })
})

describe('provider defaults', () => {
  it('只带上下文/生成/思考，不带能力', () => {
    // 服务商级没有 supportsVision 这类字段，能力只有模型级
    const out = buildProviderDefaults({
      limits: { contextBudgetTokens: '128000', temperature: '0.6' },
      thinking: { thinkingIntensity: 'medium', reasoningEffort: 'high' },
    })
    expect(out).toEqual({
      contextBudgetTokens: 128000,
      temperature: 0.6,
      thinkingIntensity: 'medium',
      reasoningEffort: 'high',
    })
    expect('supportsVision' in out).toBe(false)
  })

  it('全空时不下发任何默认', () => {
    expect(buildProviderDefaults({ limits: {}, thinking: {} })).toEqual({})
  })
})
