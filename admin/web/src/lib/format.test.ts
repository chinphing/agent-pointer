import { describe, expect, it } from 'vitest'
import { epochSeconds, fmtCount, humanSize, prettyJson, when } from './format'

describe('humanSize', () => {
  it('没有包时显示破折号，而不是 0 B', () => {
    expect(humanSize(0)).toBe('—')
    expect(humanSize(null)).toBe('—')
    expect(humanSize(undefined)).toBe('—')
  })

  it('小于 1KB 按字节显示', () => {
    expect(humanSize(1)).toBe('1 B')
    expect(humanSize(1023)).toBe('1023 B')
  })

  it('KB 保留一位小数', () => {
    expect(humanSize(1024)).toBe('1.0 KB')
    expect(humanSize(1536)).toBe('1.5 KB')
  })

  it('MB 保留一位小数', () => {
    expect(humanSize(1024 * 1024)).toBe('1.0 MB')
    expect(humanSize(20 * 1024 * 1024)).toBe('20.0 MB')
  })

  it('边界：1024 不应该落进字节分支', () => {
    expect(humanSize(1024)).not.toBe('1024 B')
  })
})

describe('when', () => {
  it('秒按本地时区换算，不是毫秒', () => {
    const ts = 1735689600 // 2025-01-01T00:00:00Z
    const d = new Date(ts * 1000)
    const p = (n: number) => String(n).padStart(2, '0')
    expect(when(ts)).toBe(
      `${d.getFullYear()}/${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
    )
  })

  it('月日时分补零，一列时间才对得齐', () => {
    // 本地时间 2025-03-05 08:07
    const d = new Date(2025, 2, 5, 8, 7, 0)
    expect(when(Math.floor(d.getTime() / 1000))).toBe('2025/03/05 08:07')
  })

  it('空值显示破折号', () => {
    expect(when(0)).toBe('—')
    expect(when(null)).toBe('—')
    expect(when(undefined)).toBe('—')
  })
})

describe('fmtCount', () => {
  it('空值当 0', () => {
    expect(fmtCount(null)).toBe('0')
    expect(fmtCount(undefined)).toBe('0')
  })

  it('加千分位', () => {
    expect(fmtCount(1234567)).toBe((1234567).toLocaleString('zh-CN'))
  })
})

describe('prettyJson', () => {
  it('合法 JSON 美化', () => {
    expect(prettyJson('{"a":1}')).toBe('{\n  "a": 1\n}')
  })

  it('非法 JSON 原样返回（审计日志可能是任意文本）', () => {
    expect(prettyJson('not json at all')).toBe('not json at all')
  })

  it('空值返回空串', () => {
    expect(prettyJson(null)).toBe('')
    expect(prettyJson('')).toBe('')
  })
})

describe('epochSeconds', () => {
  it('秒级时间戳原样返回', () => {
    expect(epochSeconds(1794417217)).toBe(1794417217)
  })

  it('毫秒级时间戳换算成秒', () => {
    // 客户端上报的 created_at 是毫秒；直接喂给 Date.parse 会 NaN，
    // 界面上就会显示成一串数字
    expect(epochSeconds(1794417217000)).toBe(1794417217)
  })

  it('ISO 字符串能解析', () => {
    expect(epochSeconds('2026-11-12T01:13:00Z')).toBe(1794445980)
  })

  it('解析不了返回 null，调用方自己决定怎么兜底', () => {
    expect(epochSeconds('不是时间')).toBeNull()
    expect(epochSeconds('')).toBeNull()
    expect(epochSeconds(null)).toBeNull()
    expect(epochSeconds(undefined)).toBeNull()
    expect(epochSeconds(0)).toBeNull()
    expect(epochSeconds(-5)).toBeNull()
  })
})
