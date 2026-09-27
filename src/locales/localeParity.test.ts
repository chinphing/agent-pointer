import { describe, expect, it } from 'vitest'
import en from './en.json'
import zhCN from './zh-CN.json'

type Json = string | number | boolean | null | Json[] | { [key: string]: Json }

function flattenKeys(value: Json, prefix = ''): string[] {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return prefix ? [prefix] : []
  }
  const keys: string[] = []
  for (const [k, v] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${k}` : k
    if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
      keys.push(...flattenKeys(v, path))
    } else {
      keys.push(path)
    }
  }
  return keys
}

describe('locale key parity', () => {
  const enKeys = new Set(flattenKeys(en as Json))
  const zhKeys = new Set(flattenKeys(zhCN as Json))

  it('every zh-CN key exists in en', () => {
    const missing = [...zhKeys].filter(k => !enKeys.has(k)).sort()
    expect(missing, `missing in en:\n${missing.join('\n')}`).toEqual([])
  })

  it('every en key exists in zh-CN', () => {
    const missing = [...enKeys].filter(k => !zhKeys.has(k)).sort()
    expect(missing, `missing in zh-CN:\n${missing.join('\n')}`).toEqual([])
  })
})
