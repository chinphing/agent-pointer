import { afterEach, describe, expect, it, vi } from 'vitest'
import { marked } from 'marked'
import {
  ensureBlankLineAfterBlockquote,
  ensureBlankLineAfterListBeforeSection,
  ensureBlankLineAfterTableRow,
  ensureBlankLinesAroundHtmlTables,
  parseMarkdown
} from './markdownConfig'
import {
  PERF_ACTIVITY_MARKDOWN_PARSE_MARKED,
  currentPerfActivity,
  resetPerf,
  setRenderPerfEnabled
} from './renderPerf'

/**
 * The pre-processing passes are skipped outright when the source cannot match
 * them, and the parse is marked in three phases. Both are white-box: the guard
 * is proved by watching `String.prototype` for the call the pass would make,
 * the phases by reading the open activity from inside `marked.parse`.
 */

/** Calls of `String.prototype.<method>` made while `run` executes. */
function countStringCalls(method: 'split' | 'replace', run: () => void): number {
  const spy = vi.spyOn(String.prototype, method)
  try {
    run()
    return spy.mock.calls.length
  } finally {
    spy.mockRestore()
  }
}

afterEach(() => {
  vi.restoreAllMocks()
  setRenderPerfEnabled(false, { persist: false })
  resetPerf()
})

describe('pre-processing guards', () => {
  it('list pass skips the line scan when no line can open with bold', () => {
    const src = '- 第一项\n- 第二项\n普通续行'
    let out = ''
    const calls = countStringCalls('split', () => {
      out = ensureBlankLineAfterListBeforeSection(src)
    })
    expect(calls).toBe(0)
    expect(out).toBe(src)
  })

  it('list pass still scans and inserts the blank line when a bold line follows', () => {
    const src = '- 第一项\n**档 2: 标题**'
    let out = ''
    const calls = countStringCalls('split', () => {
      out = ensureBlankLineAfterListBeforeSection(src)
    })
    expect(calls).toBeGreaterThan(0)
    expect(out).toBe('- 第一项\n\n**档 2: 标题**')
  })

  it('blockquote pass skips the line scan without a quote marker', () => {
    const src = '第一行\n第二行'
    let out = ''
    const calls = countStringCalls('split', () => {
      out = ensureBlankLineAfterBlockquote(src)
    })
    expect(calls).toBe(0)
    expect(out).toBe(src)
  })

  it('html-table pass skips its regexes without a table tag', () => {
    const src = '第一行\n第二行'
    let out = ''
    const calls = countStringCalls('replace', () => {
      out = ensureBlankLinesAroundHtmlTables(src)
    })
    expect(calls).toBe(0)
    expect(out).toBe(src)
  })

  it('table-row pass skips its regex without a pipe or a newline', () => {
    let noPipe = ''
    const pipeCalls = countStringCalls('replace', () => {
      noPipe = ensureBlankLineAfterTableRow('| A | B |')
    })
    expect(pipeCalls).toBe(0)
    expect(noPipe).toBe('| A | B |')

    let oneLine = ''
    const lineCalls = countStringCalls('replace', () => {
      oneLine = ensureBlankLineAfterTableRow('普通一行，没有表格')
    })
    expect(lineCalls).toBe(0)
    expect(oneLine).toBe('普通一行，没有表格')
  })

  it('table-row pass still splits a row from the line below it', () => {
    expect(ensureBlankLineAfterTableRow('| A | B |\n下一段')).toBe('| A | B |\n\n下一段')
  })

  it('every guarded pass returns a source it cannot match unchanged', () => {
    const src = '普通段落\n第二行，没有表格、引用、加粗或竖线。'
    expect(ensureBlankLineAfterListBeforeSection(src)).toBe(src)
    expect(ensureBlankLineAfterBlockquote(src)).toBe(src)
    expect(ensureBlankLinesAroundHtmlTables(src)).toBe(src)
    expect(ensureBlankLineAfterTableRow(src)).toBe(src)
  })
})

describe('parse activity markers', () => {
  it('runs marked.parse inside markdownParse:marked, inside markdownParse', () => {
    setRenderPerfEnabled(true, { persist: false })
    const seen: string[] = []
    const originalParse = marked.parse
    const spy = vi
      .spyOn(marked, 'parse')
      .mockImplementation(((src: string) => {
        seen.push(currentPerfActivity())
        return originalParse(src)
      }) as typeof marked.parse)
    try {
      const html = parseMarkdown('| X | Y |\n| --- | --- |\n| 7 | 8 |')
      expect(html).toContain('class="table-wrapper"')
      expect(seen).toEqual([PERF_ACTIVITY_MARKDOWN_PARSE_MARKED])
      // Balanced: prep, marked and wrap all closed, and so did the outer marker.
      expect(currentPerfActivity()).toBe('')
    } finally {
      spy.mockRestore()
    }
  })
})
