// @vitest-environment happy-dom

import { describe, expect, it } from 'vitest'
import {
  PAGE_SEARCH_MARK_CLASS,
  SIDEBAR_SEARCH_MARK_CLASS,
  clearSearchTextMarks,
  findTextMatchOffsets,
  highlightSearchText
} from './sidebarSearchTextHighlight'

describe('sidebar search text highlight', () => {
  it('finds all non-overlapping matches case-insensitively', () => {
    expect(findTextMatchOffsets('Pointer and POINTER again', 'pointer')).toEqual([0, 12])
  })

  it('supports Chinese phrases and trims the query', () => {
    expect(findTextMatchOffsets('精确定位到正文，再次精确定位', ' 精确定位 ')).toEqual([0, 10])
  })

  it('returns no offsets for blank or missing text', () => {
    expect(findTextMatchOffsets('some text', '   ')).toEqual([])
    expect(findTextMatchOffsets('some text', 'missing')).toEqual([])
  })

  it('marks only the active message text and clears it independently', () => {
    const root = document.createElement('div')
    root.innerHTML = [
      '<p>正文命中关键词</p>',
      '<div data-tool-call-id="tool-1">工具也命中关键词</div>',
      `<mark class="${SIDEBAR_SEARCH_MARK_CLASS}">侧边栏标记</mark>`
    ].join('')

    const result = highlightSearchText(root, '命中', {
      markClass: PAGE_SEARCH_MARK_CLASS,
      excludeSelector: '[data-tool-call-id]'
    })

    expect(result.count).toBe(1)
    expect(root.querySelectorAll(`.${PAGE_SEARCH_MARK_CLASS}`)).toHaveLength(1)
    expect(root.querySelector(`.${PAGE_SEARCH_MARK_CLASS}`)?.textContent).toBe('命中')
    expect(root.querySelector(`[data-tool-call-id] .${PAGE_SEARCH_MARK_CLASS}`)).toBeNull()
    expect(root.querySelectorAll(`.${SIDEBAR_SEARCH_MARK_CLASS}`)).toHaveLength(1)

    clearSearchTextMarks(root, PAGE_SEARCH_MARK_CLASS)
    expect(root.querySelectorAll(`.${PAGE_SEARCH_MARK_CLASS}`)).toHaveLength(0)
    expect(root.querySelectorAll(`.${SIDEBAR_SEARCH_MARK_CLASS}`)).toHaveLength(1)
    expect(root.textContent).toContain('正文命中关键词')
  })
})
