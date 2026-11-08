import { describe, expect, it } from 'vitest'
import en from './en.json'
import zhCN from './zh-CN.json'

/**
 * 两份语言包的键必须完全一致。
 *
 * 少了键不会报错，只会在界面上露出键名（或回退中文）——切到英文才被发现。
 * 客户端有同样的约束，这里保持一致。
 */
function flatten(obj: Record<string, any>, prefix = ''): string[] {
  const out: string[] = []
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object' && !Array.isArray(v)) out.push(...flatten(v, key))
    else out.push(key)
  }
  return out.sort()
}

describe('admin locale parity', () => {
  const zh = flatten(zhCN)
  const enKeys = flatten(en)

  it('两种语言的键集合相同', () => {
    expect(enKeys.filter((k) => !zh.includes(k))).toEqual([])
    expect(zh.filter((k) => !enKeys.includes(k))).toEqual([])
  })

  it('没有空字符串', () => {
    for (const [loc, pack] of [
      ['zh-CN', zhCN],
      ['en', en],
    ] as const) {
      for (const key of flatten(pack)) {
        const val = key.split('.').reduce<any>((o, k) => o?.[k], pack)
        expect(String(val).trim(), `${loc}:${key}`).not.toBe('')
      }
    }
  })
})
