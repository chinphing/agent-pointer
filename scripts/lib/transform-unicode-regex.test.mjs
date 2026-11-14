import { describe, expect, it } from 'vitest'
import { transformUnicodePropertyRegex } from './transform-unicode-regex.mjs'

/**
 * 这些用例守的是同一个底线：**产物里不能再出现 Chrome 63 解析不了的正则语法**。
 *
 * 该插件在构建期做字符串级改写，改错了不会报错、只会让目标浏览器白屏，
 * 所以每种已确认在依赖里出现的写法都要有一条用例。
 */

async function run(code) {
  const plugin = transformUnicodePropertyRegex()
  return plugin.renderChunk(code)
}

describe('transformUnicodePropertyRegex', () => {
  it('没有目标语法时原样放行（返回 null，不产生新 chunk）', async () => {
    const code = 'const answer = 42\nconsole.log(answer)'
    expect(await run(code)).toBeNull()
  })

  it('展开 Unicode 属性转义字面量', async () => {
    const code = String.raw`const r = /\p{L}+/u`
    const out = await run(code)
    expect(out).not.toBeNull()
    expect(out.code).not.toContain(String.raw`\p{`)
    expect(out.map).toBeNull()
  })

  it('展开取反形式的 Unicode 属性转义', async () => {
    const code = String.raw`const r = /\P{Nd}/u`
    const out = await run(code)
    expect(out.code).not.toContain(String.raw`\p{`)
    expect(out.code).not.toContain(String.raw`\P{`)
  })

  it('展开带脚本名的属性转义', async () => {
    const code = String.raw`const r = /\p{Script=Han}/u`
    const out = await run(code)
    expect(out.code).not.toContain(String.raw`\p{`)
  })

  it('展开命名捕获组', async () => {
    const code = String.raw`const r = /(?<year>\d{4})-(?<month>\d{2})/u`
    const out = await run(code)
    expect(out.code).not.toContain('(?<')
    expect(out.code).toContain('\\d{4}')
  })

  it('展开命名反向引用', async () => {
    const code = String.raw`const r = /(?<word>\w+)\s+\k<word>/u`
    const out = await run(code)
    expect(out.code).not.toContain('(?<')
    expect(out.code).not.toContain(String.raw`\k<`)
  })

  it('替换字符串里拼正则的片段（marked 的写法）', async () => {
    // 构建产物里这行长这样：new RegExp("\\p{P}\\p{S}", "u")
    const code = String.raw`const re = new RegExp("\\p{P}\\p{S}", "u")`
    const out = await run(code)
    expect(out.code).not.toContain(String.raw`\p{`)
    // 展开后应变成显式的字符类范围
    expect(out.code).toContain(String.raw`\u`)
  })

  it('无法降级的残留会让构建失败，而不是产出白屏包', async () => {
    // 字符串里的 \p{X} 既不是正则字面量，也不是已知的 P/S 片段
    const code = String.raw`const s = "\\p{X}"`
    await expect(run(code)).rejects.toThrow(/Chrome 63/)
  })
})
