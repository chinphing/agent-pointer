import { describe, expect, it } from 'vitest'
import {
  perMillionToYuanPerMb,
  yuanPerMbToPerMillion,
} from './billingUnits'

describe('基准价单位换算', () => {
  // 线上回归：基准价存 1 元/MB，页面却显示 1.048576 元/百万 token。
  it('1 元/MB 折成百万 token 是 0.953674，不是 1.048576', () => {
    expect(yuanPerMbToPerMillion(1)).toBeCloseTo(0.953674, 6)
  })

  it('1 元/百万 token 存进库是 1.048576 元/MB', () => {
    expect(perMillionToYuanPerMb(1)).toBeCloseTo(1.048576, 6)
  })

  it('来回换算不丢值', () => {
    for (const v of [0.5, 1, 2, 3.7, 128]) {
      expect(perMillionToYuanPerMb(yuanPerMbToPerMillion(v))).toBeCloseTo(v, 9)
    }
  })

  // 端到端对账：库里存 1.048576 元/MB，跑一百万个 token、费率 1.0，
  // 后端收费 tokens÷2²⁰×基准价 = 1.048576×0.953674 ≈ 1 元。
  it('存 1.048576 元/MB 时，一百万 token 正好收 1 元', () => {
    const stored = perMillionToYuanPerMb(1)
    const chargedPerMillion = (stored * 1_000_000) / 1_048_576
    expect(chargedPerMillion).toBeCloseTo(1, 9)
  })
})
