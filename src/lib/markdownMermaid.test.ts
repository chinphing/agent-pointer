import { describe, expect, it } from 'vitest'
import { parseMarkdown } from './markdownConfig'
import {
  isMermaidFenceLang,
  isStreamingMermaidStub,
  mermaidHostHtml,
  STREAMING_MERMAID_FENCE,
  STREAMING_MERMAID_HOST_HTML,
  STREAMING_MERMAID_STUB,
} from './markdownMermaid'
import { decodeSvgConfigAttr } from './markdownSvg'

const SAMPLE = `flowchart TD
  A[开始] --> B{是否继续?}
  B -- 是 --> C[继续]
  B -- 否 --> D[结束]`

describe('isMermaidFenceLang', () => {
  it('accepts mermaid only (case-insensitive)', () => {
    expect(isMermaidFenceLang('mermaid')).toBe(true)
    expect(isMermaidFenceLang('Mermaid')).toBe(true)
    expect(isMermaidFenceLang('svg')).toBe(false)
    expect(isMermaidFenceLang('chartjs')).toBe(false)
    expect(isMermaidFenceLang('')).toBe(false)
  })
})

describe('mermaidHostHtml', () => {
  it('encodes raw mermaid source in data-mermaid-config', () => {
    const html = mermaidHostHtml(SAMPLE)
    expect(html).toContain('class="md-mermaid')
    expect(html).toContain('data-mermaid-config=')
    const encoded = html.match(/data-mermaid-config="([^"]+)"/)?.[1]
    expect(encoded).toBeTruthy()
    expect(decodeSvgConfigAttr(encoded!)).toBe(SAMPLE)
  })

  it('includes toolbar/frame/source slots', () => {
    const html = mermaidHostHtml(SAMPLE)
    expect(html).toContain('md-mermaid-toolbar')
    expect(html).toContain('md-mermaid-frame')
    expect(html).toContain('md-mermaid-source')
  })
})

describe('isStreamingMermaidStub', () => {
  it('detects the pending stub marker', () => {
    expect(isStreamingMermaidStub(STREAMING_MERMAID_STUB)).toBe(true)
    expect(isStreamingMermaidStub(SAMPLE)).toBe(false)
  })
})

describe('parseMarkdown mermaid fences', () => {
  it('emits an md-mermaid host for a closed fence', () => {
    const html = parseMarkdown('流程如下：\n\n```mermaid\n' + SAMPLE + '\n```')
    expect(html).toContain('class="md-mermaid')
    expect(html).toContain('data-mermaid-config=')
    const encoded = html.match(/data-mermaid-config="([^"]+)"/)?.[1]
    expect(encoded).toBeTruthy()
    expect(decodeSvgConfigAttr(encoded!)).toBe(SAMPLE)
  })

  it('emits the pending host for the streaming stub', () => {
    const html = parseMarkdown(STREAMING_MERMAID_FENCE)
    expect(html).toContain('md-mermaid--pending')
    expect(html).toContain('md-mermaid-status-pending')
  })

  it('keeps non-mermaid code fences as code blocks', () => {
    const html = parseMarkdown('```js\nconst a = 1\n```')
    expect(html).not.toContain('md-mermaid')
    expect(html).toContain('code-block')
    expect(html).toContain('fence-block-lang')
    expect(html).toContain('>js</div>')
  })

  it('stubs only incomplete fences while streaming', () => {
    const open = parseMarkdown('前文\n\n```mermaid\nflowchart TD\n  A[开始]\n', {
      streamingMermaid: true,
    })
    expect(open).toContain('md-mermaid--pending')

    const closed = parseMarkdown(
      '前文\n\n```mermaid\n' + SAMPLE + '\n```\n\n后文…',
      { streamingMermaid: true }
    )
    expect(closed).toContain('class="md-mermaid')
    expect(closed).not.toContain('md-mermaid--pending')
  })
})
