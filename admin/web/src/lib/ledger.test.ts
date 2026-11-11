import { describe, expect, it } from 'vitest'
import { ledgerKindLabel } from './ledger'

const dict: Record<string, string> = {
  'me.kindGrant': '赠送',
  'me.kindCharge': '扣费',
  'me.kindRecharge': '充值',
}
const t = (key: string) => dict[key] ?? key

describe('ledgerKindLabel', () => {
  it('已知类型走中文标签', () => {
    expect(ledgerKindLabel('grant', t)).toBe('赠送')
    expect(ledgerKindLabel('charge', t)).toBe('扣费')
    // 充值不能显示成「赠送」：那是用户付的钱，账目上必须分得清
    expect(ledgerKindLabel('recharge', t)).toBe('充值')
  })

  it('未知类型原样显示，不吞成空白', () => {
    // 后端加新类型时页面要能显示出来，而不是一行空白让人以为渲染坏了。
    expect(ledgerKindLabel('refund', t)).toBe('refund')
    expect(ledgerKindLabel('', t)).toBe('')
  })
})
