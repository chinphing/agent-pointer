/**
 * 流水类型的展示映射。
 *
 * 开源版后端只有 `grant` / `charge` 两种（见 `admin/src/billing.rs` 的 KIND_*）；
 * 扩展（私有仓）可以补自己的类型，见 `ext.ts` 的 `ledgerKindKeys`。
 * 未知值原样返回，不要吞成空白 —— 后端加新类型时页面还能显示出来，
 * 而不是变成一行空白让人以为是渲染坏了。
 */
import { ledgerKindKeys } from '../ext'

const KIND_KEYS: Record<string, string> = {
  grant: 'me.kindGrant',
  charge: 'me.kindCharge',
  ...ledgerKindKeys,
}

export function ledgerKindLabel(kind: string, t: (key: string) => string): string {
  const key = KIND_KEYS[kind]
  return key ? t(key) : kind
}
