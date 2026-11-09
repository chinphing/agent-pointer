import { describe, expect, it } from 'vitest'
import { parseRatioInput } from './ratioInput'

describe('parseRatioInput', () => {
  // 回归：`<input type="number" v-model>` 给的是 Number，
  // 旧代码对它调 `.trim()` 抛 TypeError，保存请求根本没发出去。
  it('接受数字（v-model 在 type=number 上的真实类型）', () => {
    expect(parseRatioInput(0.1)).toEqual({ ok: true, value: 0.1 })
    expect(parseRatioInput(1)).toEqual({ ok: true, value: 1 })
    expect(parseRatioInput(0)).toEqual({ ok: true, value: 0 })
  })

  it('也接受字符串（未输入时的初值、手填）', () => {
    expect(parseRatioInput('0.1')).toEqual({ ok: true, value: 0.1 })
    expect(parseRatioInput(' 2 ')).toEqual({ ok: true, value: 2 })
  })

  it('空值单独区分：提示「请填费率」而不是「非法」', () => {
    expect(parseRatioInput('')).toMatchObject({ ok: false, reason: 'empty' })
    expect(parseRatioInput('   ')).toMatchObject({ ok: false, reason: 'empty' })
    expect(parseRatioInput(null)).toMatchObject({ ok: false, reason: 'empty' })
    expect(parseRatioInput(undefined)).toMatchObject({ ok: false, reason: 'empty' })
  })

  it('非法值被拒', () => {
    expect(parseRatioInput('abc')).toMatchObject({ ok: false, reason: 'invalid' })
    expect(parseRatioInput(-1)).toMatchObject({ ok: false, reason: 'invalid' })
    expect(parseRatioInput(Number.NaN)).toMatchObject({ ok: false, reason: 'invalid' })
    expect(parseRatioInput(Number.POSITIVE_INFINITY)).toMatchObject({
      ok: false,
      reason: 'invalid',
    })
  })
})
