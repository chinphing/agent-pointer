/**
 * 图表用的数值刻度计算。
 *
 * 原来的柱状图直接把 `tokens` 除以最大值当高度，没有任何刻度：读者只能看
 * 「谁比谁高」，读不出具体数量，也分不清「10 万」和「100 万」差多少。
 * 这里把「好看的刻度」抽成纯函数，单测覆盖边界。
 */

/** 大数字压缩成人类可读：1234 → 1.2K，2602500 → 2.6M。 */
export function fmtCompact(n: number | null | undefined): string {
  const v = Number(n || 0)
  if (!Number.isFinite(v)) return '0'
  const abs = Math.abs(v)
  const sign = v < 0 ? '-' : ''
  if (abs < 1000) return `${sign}${Math.round(abs)}`
  if (abs < 1_000_000) return `${sign}${trim(abs / 1000)}K`
  if (abs < 1_000_000_000) return `${sign}${trim(abs / 1_000_000)}M`
  return `${sign}${trim(abs / 1_000_000_000)}B`
}

/** 保留一位小数，但 1.0 → 1（避免「1.0K」这种冗余写法）。 */
function trim(v: number): string {
  const s = v.toFixed(1)
  return s.endsWith('.0') ? s.slice(0, -2) : s
}

/**
 * 把最大值抬到「好看的整数」，让刻度落在 0 / 250K / 500K 这类值上，
 * 而不是 0 / 228125 / 456250。
 */
export function niceMax(value: number): number {
  const v = Number(value) || 0
  if (v <= 0) return 1
  const exp = Math.floor(Math.log10(v))
  const base = Math.pow(10, exp)
  for (const mult of [1, 2, 2.5, 5, 10]) {
    const candidate = mult * base
    // 留一点浮点余量，避免 100 因精度算成 100.00000001 而跳到下一档
    if (candidate >= v * (1 - 1e-9)) return candidate
  }
  return 10 * base
}

export interface Tick {
  /** 刻度值 */
  value: number
  /** 已格式化的标签 */
  label: string
  /** 距底部的百分比（0 在底、100 在顶） */
  pct: number
}

/**
 * 生成从 0 到 top 的等分刻度（含 0 和 top）。
 * `divisions` 是段数，返回 divisions+1 个刻度。
 */
export function makeTicks(max: number, divisions = 4): Tick[] {
  const top = niceMax(max)
  const n = Math.max(1, Math.floor(divisions))
  const out: Tick[] = []
  for (let i = 0; i <= n; i++) {
    const value = (top / n) * i
    out.push({ value, label: fmtCompact(value), pct: (i / n) * 100 })
  }
  return out
}
