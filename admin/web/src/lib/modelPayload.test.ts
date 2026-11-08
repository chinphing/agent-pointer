import { describe, expect, it } from 'vitest'
import {
  buildModelPayload,
  buildProviderDefaults,
  intensityFromBudget,
  payloadToState,
} from './modelPayload'

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

  it('不产出 enableThinking / reasoningEffort —— 都由强度推出来', () => {
    // 设了强度，客户端 round_enable_thinking = 强度非 off 即 true，
    // round_reasoning_effort = effort_for_intensity(level)，存的值被覆盖
    const out = buildModelPayload({
      caps: {},
      limits: {},
      thinking: { enableThinking: 'false', reasoningEffort: 'max', thinkingIntensity: 'low' },
    })
    expect(out).toEqual({ thinkingIntensity: 'low' })
  })

  it('思考只下发强度与回传开关，其余原始字段不展开', () => {
    // 管理端只存原始值，不做换算：强度 → 预算/effort 是客户端的事
    const out = buildModelPayload({
      caps: {},
      limits: {},
      thinking: { thinkingIntensity: 'high', reasoningInMessages: 'true' },
    })
    expect(out).toEqual({ thinkingIntensity: 'high', reasoningInMessages: true })
    expect('thinkingBudget' in out).toBe(false)
    expect('reasoningEffort' in out).toBe(false)
    expect('enableThinking' in out).toBe(false)
  })

  it('历史字段按客户端顺序反推强度', () => {
    // resolve_thinking_intensity：强度 → effort(high/max) → 预算 → enable
    expect(payloadToState({ reasoningEffort: 'max' }).thinking.thinkingIntensity).toBe('max')
    expect(payloadToState({ reasoningEffort: 'high' }).thinking.thinkingIntensity).toBe('high')
    expect(payloadToState({ enableThinking: false }).thinking.thinkingIntensity).toBe('off')
    expect(payloadToState({ enableThinking: true }).thinking.thinkingIntensity).toBe('medium')
    // 强度优先于其他字段
    expect(
      payloadToState({ thinkingIntensity: 'low', reasoningEffort: 'max', enableThinking: false }).thinking
        .thinkingIntensity
    ).toBe('low')
  })

  it('不产出 thinkingBudget —— 预算由强度换算，不单独下发', () => {
    const out = buildModelPayload({
      caps: {},
      limits: {},
      thinking: { thinkingBudget: '4096', thinkingIntensity: 'high' },
    })
    expect('thinkingBudget' in out).toBe(false)
    expect(out.thinkingIntensity).toBe('high')
  })

  it('老数据只有预算时换算成强度', () => {
    // 客户端 intensity_from_budget 的分档：<1536 低 / <3072 中 / <6144 高 / 其余最高
    expect(payloadToState({ thinkingBudget: 8192 }).thinking.thinkingIntensity).toBe('max')
    expect(payloadToState({ thinkingBudget: 4096 }).thinking.thinkingIntensity).toBe('high')
    expect(payloadToState({ thinkingBudget: 2048 }).thinking.thinkingIntensity).toBe('medium')
    expect(payloadToState({ thinkingBudget: 1024 }).thinking.thinkingIntensity).toBe('low')
    expect(payloadToState({ thinkingBudget: 0 }).thinking.thinkingIntensity).toBe('off')
  })

  it('强度已有值时不被预算覆盖', () => {
    expect(
      payloadToState({ thinkingIntensity: 'low', thinkingBudget: 8192 }).thinking.thinkingIntensity
    ).toBe('low')
  })

  it('intensityFromBudget 与客户端分档一致', () => {
    expect(intensityFromBudget(0)).toBe('off')
    expect(intensityFromBudget(1024)).toBe('low')
    expect(intensityFromBudget(2048)).toBe('medium')
    expect(intensityFromBudget(4096)).toBe('high')
    expect(intensityFromBudget(8192)).toBe('max')
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
      reasoningInMessages: false,
    }
    const state = payloadToState(payload)
    expect(state.caps.supportsVision).toBe('true')
    expect(state.caps.supportsAudio).toBe('')
    expect(state.limits.contextBudgetTokens).toBe('131072')
    expect(state.thinking.thinkingIntensity).toBe('high')
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
      thinking: { thinkingIntensity: 'medium' },
    })
    expect(out).toEqual({
      contextBudgetTokens: 128000,
      temperature: 0.6,
      thinkingIntensity: 'medium',
    })
    expect('supportsVision' in out).toBe(false)
  })

  it('全空时不下发任何默认', () => {
    expect(buildProviderDefaults({ limits: {}, thinking: {} })).toEqual({})
  })
})
