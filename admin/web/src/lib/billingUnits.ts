/**
 * 基准价的单位换算。
 *
 * 后端存的是**元 / MB**（官网口径：1 MB = 2²⁰ token），计费公式是
 * `tokens ÷ 2²⁰ × 基准价 × 费率`。计费页按「元 / 百万 token」展示，
 * 两者差 2²⁰ 与 10⁶ 之比（1.048576）。
 *
 * 方向很容易搞反：1 元 / MB 是「1,048,576 个 token 收 1 元」，
 * 折成一百万个 token 只收 0.953674 元——所以 MB → 百万是**除** 1.048576。
 */

export const TOKENS_PER_MB = 1_048_576
export const TOKENS_PER_MILLION = 1_000_000

/** 元 / MB → 元 / 百万 token。1 元/MB = 0.953674 元/百万。 */
export function yuanPerMbToPerMillion(yuanPerMb: number): number {
  return (yuanPerMb * TOKENS_PER_MILLION) / TOKENS_PER_MB
}

/** 元 / 百万 token → 元 / MB。1 元/百万 = 1.048576 元/MB。 */
export function perMillionToYuanPerMb(yuanPerMillion: number): number {
  return (yuanPerMillion * TOKENS_PER_MB) / TOKENS_PER_MILLION
}
