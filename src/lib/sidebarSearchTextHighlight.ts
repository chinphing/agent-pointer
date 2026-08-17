export const SIDEBAR_SEARCH_MARK_CLASS = 'sidebar-search-text-mark'
export const PAGE_SEARCH_MARK_CLASS = 'page-search-text-mark'
export const SIDEBAR_SEARCH_HIGHLIGHT_NAME = 'sidebar-search-text'

export type SearchTextHighlightOptions = {
  markClass?: string
  excludeSelector?: string
}

type HighlightRegistry = {
  set(name: string, highlight: Highlight): void
  delete(name: string): boolean
}

type CssWithHighlights = typeof CSS & { highlights?: HighlightRegistry }

function normalizedQuery(query: string): string {
  return query.trim().toLocaleLowerCase()
}

/** Case-insensitive match offsets used by the DOM marker and unit tests. */
export function findTextMatchOffsets(text: string, query: string): number[] {
  const needle = normalizedQuery(query)
  if (!needle) return []

  const haystack = text.toLocaleLowerCase()
  const offsets: number[] = []
  let from = 0
  while (from <= haystack.length - needle.length) {
    const index = haystack.indexOf(needle, from)
    if (index < 0) break
    offsets.push(index)
    from = index + needle.length
  }
  return offsets
}

function eligibleTextNodes(
  root: HTMLElement,
  { excludeSelector }: SearchTextHighlightOptions = {}
): Text[] {
  const nodes: Text[] = []
  const view = root.ownerDocument.defaultView
  const nodeFilter = view?.NodeFilter ?? NodeFilter
  const walker = root.ownerDocument.createTreeWalker(root, nodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const parent = node.parentElement
      if (!parent || !node.nodeValue) return nodeFilter.FILTER_REJECT
      if (parent.closest(`.${SIDEBAR_SEARCH_MARK_CLASS}, .${PAGE_SEARCH_MARK_CLASS}, script, style`)) {
        return nodeFilter.FILTER_REJECT
      }
      if (excludeSelector && parent.closest(excludeSelector)) {
        return nodeFilter.FILTER_REJECT
      }
      return nodeFilter.FILTER_ACCEPT
    }
  })
  while (walker.nextNode()) nodes.push(walker.currentNode as Text)
  return nodes
}

/** Build ranges against concatenated rendered text, including matches split across Markdown nodes. */
export function findRenderedTextRanges(
  root: HTMLElement,
  query: string,
  options: SearchTextHighlightOptions = {}
): Range[] {
  const needle = query.trim()
  if (!needle) return []

  const nodes = eligibleTextNodes(root, options)
  const spans: Array<{ node: Text; start: number; end: number }> = []
  let text = ''
  for (const node of nodes) {
    const start = text.length
    text += node.nodeValue ?? ''
    spans.push({ node, start, end: text.length })
  }

  const ranges: Range[] = []
  for (const offset of findTextMatchOffsets(text, needle)) {
    const endOffset = offset + needle.length
    const startSpan = spans.find(span => span.start <= offset && offset < span.end)
    const endSpan = spans.find(span => span.start < endOffset && endOffset <= span.end)
    if (!startSpan || !endSpan) continue

    const range = root.ownerDocument.createRange()
    range.setStart(startSpan.node, offset - startSpan.start)
    range.setEnd(endSpan.node, endOffset - endSpan.start)
    ranges.push(range)
  }
  return ranges
}

function unwrapMark(mark: HTMLElement) {
  const parent = mark.parentNode
  if (!parent) return
  while (mark.firstChild) parent.insertBefore(mark.firstChild, mark)
  parent.removeChild(mark)
  parent.normalize()
}

/** Remove DOM marks for one search surface without disturbing another. */
export function clearSearchTextMarks(
  root: ParentNode,
  markClass = SIDEBAR_SEARCH_MARK_CLASS
): void {
  root.querySelectorAll<HTMLElement>(`.${markClass}`).forEach(unwrapMark)
}

/** Remove precise sidebar-search highlights and fallback marks. */
export function clearSidebarSearchTextMarks(root: ParentNode): void {
  ;(CSS as CssWithHighlights).highlights?.delete(SIDEBAR_SEARCH_HIGHLIGHT_NAME)
  clearSearchTextMarks(root, SIDEBAR_SEARCH_MARK_CLASS)
}

function fallbackMarkRanges(
  root: HTMLElement,
  query: string,
  options: SearchTextHighlightOptions = {}
): HTMLElement[] {
  const needle = query.trim()
  const markClass = options.markClass ?? SIDEBAR_SEARCH_MARK_CLASS
  const marks: HTMLElement[] = []
  for (const original of eligibleTextNodes(root, options)) {
    const text = original.nodeValue ?? ''
    const offsets = findTextMatchOffsets(text, needle)
    if (offsets.length === 0) continue

    let cursor = original
    let consumed = 0
    for (const offset of offsets) {
      const relativeOffset = offset - consumed
      if (relativeOffset > 0) cursor = cursor.splitText(relativeOffset)
      const tail = cursor.splitText(needle.length)
      const mark = root.ownerDocument.createElement('mark')
      mark.className = markClass
      // Keep the visual treatment self-contained: older WebKit builds can expose
      // CSS Custom Highlight APIs without painting ::highlight() reliably.
      mark.style.setProperty('color', 'inherit')
      mark.style.setProperty('background', 'hsl(var(--search-mark) / 0.5)')
      mark.style.setProperty('border-radius', '0.2rem')
      mark.style.setProperty('box-shadow', '0 0 0 1px hsl(var(--search-mark) / 0.28)')
      mark.style.setProperty('padding', '0 0.08em')
      cursor.parentNode?.insertBefore(mark, cursor)
      mark.appendChild(cursor)
      marks.push(mark)
      cursor = tail
      consumed = offset + needle.length
    }
  }
  return marks
}

/** Highlight rendered text with an isolated mark class. */
export function highlightSearchText(
  root: HTMLElement,
  query: string,
  options: SearchTextHighlightOptions = {}
): { count: number; scrollTarget: Element | null } {
  const markClass = options.markClass ?? SIDEBAR_SEARCH_MARK_CLASS
  clearSearchTextMarks(root, markClass)
  const ranges = findRenderedTextRanges(root, query, options)
  if (ranges.length === 0) return { count: 0, scrollTarget: null }

  // Always use real DOM marks here. Some WKWebView versions expose
  // CSS.highlights and accept registrations but never paint ::highlight().
  const marks = fallbackMarkRanges(root, query, options)
  return { count: marks.length, scrollTarget: marks[0] ?? null }
}

/** Highlight sidebar-search text and return the first precise scroll target. */
export function highlightSidebarSearchText(
  root: HTMLElement,
  query: string
): { count: number; scrollTarget: Element | null } {
  clearSidebarSearchTextMarks(root)
  return highlightSearchText(root, query, { markClass: SIDEBAR_SEARCH_MARK_CLASS })
}
