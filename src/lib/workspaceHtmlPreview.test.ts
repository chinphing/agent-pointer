import { describe, expect, it } from 'vitest'
import {
  buildHtmlPreviewSrcdoc,
  HTML_PREVIEW_SANDBOX,
  isHtmlPreviewPath,
  visibleIframeOffsetWidth
} from './workspaceHtmlPreview'

describe('workspace HTML preview', () => {
  it('maps html paths through the shared preview registry', () => {
    expect(isHtmlPreviewPath('page.html')).toBe(true)
    expect(isHtmlPreviewPath('index.HTM')).toBe(true)
    expect(isHtmlPreviewPath('notes.md')).toBe(false)
  })

  it('allows scripts in the sandbox without app origin', () => {
    expect(HTML_PREVIEW_SANDBOX).toContain('allow-scripts')
    expect(HTML_PREVIEW_SANDBOX).not.toContain('allow-same-origin')
  })

  it('wraps a fragment and keeps inline scripts', () => {
    const srcdoc = buildHtmlPreviewSrcdoc('<h1>Hi</h1><script>window.__demo = 1</script>')
    expect(srcdoc).toContain('<h1>Hi</h1>')
    expect(srcdoc).toContain('window.__demo = 1')
    expect(srcdoc).toContain('<!DOCTYPE html>')
    expect(srcdoc).toContain('charset="utf-8"')
  })

  it('injects into an existing head', () => {
    const srcdoc = buildHtmlPreviewSrcdoc(
      '<!DOCTYPE html><html><head><title>T</title></head><body>Ok</body></html>'
    )
    expect(srcdoc).toMatch(/<head[^>]*>[\s\S]*charset="utf-8"[\s\S]*<title>T<\/title>/i)
    expect(srcdoc).toContain('Ok')
  })

  it('reads only visible iframe widths', () => {
    expect(visibleIframeOffsetWidth(null)).toBeNull()
    const root = {
      querySelectorAll: () => [{ offsetWidth: 0 }, { offsetWidth: 480 }]
    } as unknown as ParentNode
    expect(visibleIframeOffsetWidth(root)).toBe(480)
  })
})
