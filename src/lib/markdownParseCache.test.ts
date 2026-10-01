import { afterEach, describe, expect, it, vi } from 'vitest'
import { marked } from 'marked'
import {
  MARKDOWN_PARSE_CACHE_MAX_ENTRIES,
  MARKDOWN_PARSE_CACHE_MAX_SOURCE_CHARS,
  parseMarkdown,
  type ParseMarkdownOptions
} from './markdownConfig'

/**
 * `parseMarkdown` caches per source + streaming flags, so a message that the
 * virtualizer unmounts and remounts is not parsed again. These tests pin the
 * hit, the two bounds, the eviction order and the flag separation.
 */

/** Unique every call, so a filler insert can never hit an existing entry. */
let fillerSeq = 0

function parseFiller(): void {
  fillerSeq += 1
  parseMarkdown(`cache-filler-${fillerSeq}`)
}

/**
 * Push the cache past its entry bound with unique sources, so the entry under
 * test is gone whatever earlier tests cached.
 */
function evictEverything(): void {
  for (let i = 0; i <= MARKDOWN_PARSE_CACHE_MAX_ENTRIES; i += 1) parseFiller()
}

/** Sources that exercise every pre-processing pass and every fence renderer. */
const REPRESENTATIVE_SOURCES: string[] = [
  '| A | B |\n| --- | --- |\n| 1 | 2 |',
  '<table><tr><td>a</td></tr></table>\n## 下一节',
  '> 引用一行\n没有 > 的续行',
  '- 第一项\n- 第二项\n**档 2: 标题**',
  '```ts\nconst a = 1\n```',
  '```chartjs\n{"type":"bar","data":{"labels":["A"],"datasets":[{"data":[1]}]}}\n```',
  '```svg\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4"/></svg>\n```',
  '```mermaid\nflowchart TD\n  A[开始] --> B[结束]\n```',
  '普通段落，没有表格、引用、列表或围栏。',
  '',
  '   \n\n  '
]

afterEach(() => {
  vi.restoreAllMocks()
})

describe('parseMarkdown cache', () => {
  it('parses a repeated source once and returns the same HTML', () => {
    const parseSpy = vi.spyOn(marked, 'parse')
    const src = '重复解析的段落，带 **加粗** 与 `行内代码`。'

    const before = parseSpy.mock.calls.length
    const first = parseMarkdown(src)
    expect(parseSpy.mock.calls.length).toBe(before + 1)

    const second = parseMarkdown(src)
    expect(parseSpy.mock.calls.length).toBe(before + 1)
    expect(second).toBe(first)
  })

  it('returns byte-identical HTML when the cached entry has been evicted', () => {
    const parseSpy = vi.spyOn(marked, 'parse')
    for (const src of REPRESENTATIVE_SOURCES) {
      // Blank sources never reach the pipeline, so there is nothing to compare.
      if (!src.trim()) continue
      const cached = parseMarkdown(src)
      evictEverything()
      const before = parseSpy.mock.calls.length
      const fresh = parseMarkdown(src)
      // Re-parsed (the entry really was gone) …
      expect(parseSpy.mock.calls.length).toBe(before + 1)
      // … and produced the same bytes as the cached one.
      expect(fresh).toBe(cached)
    }
  })

  it('does not parse an empty or blank source at all', () => {
    const parseSpy = vi.spyOn(marked, 'parse')
    const before = parseSpy.mock.calls.length
    expect(parseMarkdown('')).toBe('')
    expect(parseMarkdown('   \n\n  ')).toBe('')
    expect(parseSpy.mock.calls.length).toBe(before)
  })

  it('never serves a streaming parse for a call without the flag', () => {
    const cases: Array<{ src: string; option: ParseMarkdownOptions }> = [
      {
        src: '```chartjs\n{"type":"bar","data":{"labels":["A"],"datasets":[{"data":[1]}]}}\n```',
        option: { streamingCharts: true }
      },
      {
        src: '```svg\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5"\n',
        option: { streamingSvgs: true }
      },
      {
        src: '```mermaid\nflowchart TD\n  A[开始] --> B[结束]\n',
        option: { streamingMermaid: true }
      }
    ]
    for (const { src, option } of cases) {
      const streaming = parseMarkdown(src, option)
      const plain = parseMarkdown(src)
      expect(plain).not.toBe(streaming)
      // Each variant keeps its own entry, in both directions.
      expect(parseMarkdown(src, option)).toBe(streaming)
      expect(parseMarkdown(src)).toBe(plain)
    }
  })

  it('evicts the oldest entry once the entry bound is crossed', () => {
    const parseSpy = vi.spyOn(marked, 'parse')
    evictEverything()
    const oldest = 'eviction-oldest **加粗**'
    parseMarkdown(oldest)

    // Exactly the bound in fresh entries: the oldest one has to be gone.
    for (let i = 0; i < MARKDOWN_PARSE_CACHE_MAX_ENTRIES; i += 1) parseFiller()

    const before = parseSpy.mock.calls.length
    parseMarkdown(oldest)
    expect(parseSpy.mock.calls.length).toBe(before + 1)
  })

  it('drops the least recently used entry, not the one re-used most recently', () => {
    const parseSpy = vi.spyOn(marked, 'parse')
    evictEverything()
    const reused = 'lru-reused **加粗**'
    const dropped = 'lru-dropped **加粗**'
    parseMarkdown(reused)
    parseMarkdown(dropped)
    // A hit re-inserts `reused` behind `dropped`: it is now the newer entry.
    parseMarkdown(reused)

    for (let i = 0; i < MARKDOWN_PARSE_CACHE_MAX_ENTRIES - 1; i += 1) parseFiller()

    const beforeReused = parseSpy.mock.calls.length
    parseMarkdown(reused)
    expect(parseSpy.mock.calls.length).toBe(beforeReused)

    const beforeDropped = parseSpy.mock.calls.length
    parseMarkdown(dropped)
    expect(parseSpy.mock.calls.length).toBe(beforeDropped + 1)
  })

  it('drops entries by the source-character budget', () => {
    const parseSpy = vi.spyOn(marked, 'parse')
    const half = Math.floor(MARKDOWN_PARSE_CACHE_MAX_SOURCE_CHARS / 2) + 1
    const big = (tag: string): string => `第${tag}段。` + 'x'.repeat(half)
    const first = big('一')
    const second = big('二')
    const third = big('三')

    parseMarkdown(first)
    parseMarkdown(second)
    parseMarkdown(third)

    // Any two of these exceed the budget, so only the newest one survives.
    const beforeThird = parseSpy.mock.calls.length
    parseMarkdown(third)
    expect(parseSpy.mock.calls.length).toBe(beforeThird)

    const beforeFirst = parseSpy.mock.calls.length
    parseMarkdown(first)
    expect(parseSpy.mock.calls.length).toBe(beforeFirst + 1)
  })

  it('leaves a source larger than the whole budget uncached', () => {
    const parseSpy = vi.spyOn(marked, 'parse')
    const huge = 'x'.repeat(MARKDOWN_PARSE_CACHE_MAX_SOURCE_CHARS + 1)
    const before = parseSpy.mock.calls.length
    parseMarkdown(huge)
    parseMarkdown(huge)
    expect(parseSpy.mock.calls.length).toBe(before + 2)
  })
})
