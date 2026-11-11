/**
 * 流水类型的展示映射。
 *
 * 开源版后端只有 `grant` / `charge` 两种（见 `admin/src/billing.rs` 的 KIND_*）；
 * 扩展（私有仓）可以补自己的类型，见 `ext.ts` 的 `ledgerKindKeys`。
 * 未知值原样返回，不要吞成空白 —— 后端加新类型时页面还能显示出来，
 * 而不是变成一行空白让人以为是渲染坏了。
 */
// 必须走别名：相对路径会绕过 vite 的 @ext 替换，私有构建时静默拿到开源仓那份
// 空实现，界面上就会直接显示 `recharge` 这样的原始 kind。
import { ledgerKindKeys } from '@ext'

const KIND_KEYS: Record<string, string> = {
  grant: 'me.kindGrant',
  charge: 'me.kindCharge',
  ...ledgerKindKeys,
}

export function ledgerKindLabel(kind: string, t: (key: string) => string): string {
  const key = KIND_KEYS[kind]
  return key ? t(key) : kind
}
