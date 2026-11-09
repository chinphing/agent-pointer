/**
 * 费率输入解析。
 *
 * `<input type="number" v-model>` 绑出来的值是 **Number**，不是字符串——
 * 直接对它调 `.trim()` 会抛 `TypeError`，点击保存时整个处理函数中断，
 * 请求根本发不出去，界面上也不报错（只有控制台有）。
 *
 * 所以解析统一走这里，输入是什么类型都安全。
 */
export interface RatioParse {
  ok: boolean
  /** 仅在 `ok` 为真时有意义。 */
  value: number
  /** 仅在 `ok` 为假时有值。 */
  reason?: 'empty' | 'invalid'
}

export function parseRatioInput(raw: unknown): RatioParse {
  if (raw === null || raw === undefined) return { ok: false, value: 0, reason: 'empty' }
  if (typeof raw === 'number') {
    if (!Number.isFinite(raw)) return { ok: false, value: 0, reason: 'invalid' }
    return raw < 0 ? { ok: false, value: 0, reason: 'invalid' } : { ok: true, value: raw }
  }
  const text = String(raw).trim()
  if (!text) return { ok: false, value: 0, reason: 'empty' }
  const value = Number(text)
  if (!Number.isFinite(value) || value < 0) return { ok: false, value: 0, reason: 'invalid' }
  return { ok: true, value }
}
