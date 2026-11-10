/**
 * 流水类型的展示映射。
 *
 * 后端只有 `grant` / `charge` 两种（见 `admin/src/billing.rs` 的 KIND_*）。
 * 未知值原样返回，不要吞成空白 —— 后端加新类型时页面还能显示出来，
 * 而不是变成一行空白让人以为是渲染坏了。
 */
const KIND_KEYS: Record<string, string> = {
  grant: 'me.kindGrant',
  charge: 'me.kindCharge',
}

export function ledgerKindLabel(kind: string, t: (key: string) => string): string {
  const key = KIND_KEYS[kind]
  return key ? t(key) : kind
}
