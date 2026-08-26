// @vitest-environment happy-dom

import { describe, expect, it } from 'vitest'
import { parseMarkdown } from './markdownConfig'
import { canvasRgb } from './markdownChart'
import {
  isMermaidFenceLang,
  isStreamingMermaidStub,
  mermaidHostHtml,
  mermaidInitializeConfig,
  mermaidThemeCacheKey,
  mermaidThemeScheme,
  mermaidThemeVariables,
  STREAMING_MERMAID_FENCE,
  STREAMING_MERMAID_HOST_HTML,
  STREAMING_MERMAID_STUB,
  stripMermaidHostThemeOverrides,
  roundMermaidSvgRects,
  MERMAID_NODE_RX,
  MERMAID_CLUSTER_RX,
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

describe('mermaid host theme', () => {
  function paintScheme(mode: 'light' | 'dark') {
    const root = document.documentElement
    root.classList.remove('light', 'dark')
    root.classList.add(mode)
    if (mode === 'dark') {
      root.style.setProperty('--foreground', '240 6% 96%')
      root.style.setProperty('--card', '240 4% 11%')
      root.style.setProperty('--fence-bg', '240 4% 8%')
      root.style.setProperty('--accent', '211 100% 58%')
      root.style.setProperty('--danger', '4 72% 58%')
      root.style.setProperty('--mermaid-cluster', '240 5% 16%')
      root.style.setProperty('--mermaid-node', '240 5% 22%')
      root.style.setProperty('--mermaid-node-border', '240 8% 42%')
      root.style.setProperty('--mermaid-edge', '240 6% 72%')
    } else {
      root.style.setProperty('--foreground', '240 6% 10%')
      root.style.setProperty('--card', '0 0% 100%')
      root.style.setProperty('--fence-bg', '0 0% 100%')
      root.style.setProperty('--accent', '211 100% 46%')
      root.style.setProperty('--danger', '4 78% 50%')
      root.style.setProperty('--mermaid-cluster', '240 5% 96%')
      root.style.setProperty('--mermaid-node', '0 0% 100%')
      root.style.setProperty('--mermaid-node-border', '240 8% 82%')
      root.style.setProperty('--mermaid-edge', '240 6% 68%')
    }
  }

  it('maps CSS tokens and follows html.dark', () => {
    paintScheme('dark')
    expect(mermaidThemeScheme()).toBe('dark')
    const vars = mermaidThemeVariables()
    expect(vars.darkMode).toBe(true)
    expect(vars.clusterBkg).toBe(canvasRgb('--mermaid-cluster', '240 5% 16%'))
    expect(vars.nodeBkg).toBe(canvasRgb('--mermaid-node', '240 5% 22%'))
    expect(vars.primaryTextColor).toBe(canvasRgb('--foreground', '240 6% 96%'))
    expect(vars.lineColor).toBe(canvasRgb('--mermaid-edge', '240 6% 72%'))
    expect(vars.nodeBorder).toBe(canvasRgb('--mermaid-node-border', '240 8% 42%'))
    expect(vars.strokeWidth).toBe(1.5)
    expect(vars.nodeBkg).not.toBe(canvasRgb('--card', '240 4% 11%'))
    expect(vars.background).toBe(canvasRgb('--fence-bg', '240 4% 8%'))
    expect(String(vars.clusterBkg)).not.toMatch(/#fff4dd|#ffffde/i)
    expect(mermaidThemeCacheKey('abc')).toBe('dark|abc')

    paintScheme('light')
    expect(mermaidThemeScheme()).toBe('light')
    const light = mermaidThemeVariables()
    expect(light.darkMode).toBe(false)
    expect(light.clusterBkg).toBe(canvasRgb('--mermaid-cluster', '240 5% 96%'))
    expect(light.nodeBkg).toBe(canvasRgb('--mermaid-node', '0 0% 100%'))
    expect(light.background).toBe(canvasRgb('--fence-bg', '0 0% 100%'))
    expect(light.nodeBorder).toBe(canvasRgb('--mermaid-node-border', '240 8% 82%'))
    expect(light.lineColor).toBe(canvasRgb('--mermaid-edge', '240 6% 68%'))
    expect(mermaidInitializeConfig().theme).toBe('base')
    expect(mermaidInitializeConfig().htmlLabels).toBe(false)
    expect(mermaidInitializeConfig().flowchart.htmlLabels).toBe(false)
  })

  it('strips init directives and keeps the diagram body', () => {
    const raw = `%%{init: {'theme':'dark', 'themeVariables': {'primaryColor':'#fff4dd'}}}%%\nflowchart TD\n  A[开始] --> B[结束]`
    const stripped = stripMermaidHostThemeOverrides(raw)
    expect(stripped).toContain('flowchart TD')
    expect(stripped).toContain('A[开始]')
    expect(stripped).not.toMatch(/%%\{/)
    expect(stripped).not.toContain('#fff4dd')
  })

  it('rounds flowchart node and cluster rects', () => {
    const raw = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 80">
  <g class="node"><rect width="80" height="32" class="basic label-container"/></g>
  <g class="cluster"><rect width="180" height="70"/></g>
  <g class="node"><rect class="text" width="40" height="12"/></g>
  <g class="edgeLabel"><rect width="20" height="10"/></g>
</svg>`
    const rounded = roundMermaidSvgRects(raw)
    expect(rounded).toContain(`rx="${MERMAID_NODE_RX}"`)
    expect(rounded).toContain(`ry="${MERMAID_NODE_RX}"`)
    expect(rounded).toContain(`rx="${MERMAID_CLUSTER_RX}"`)
    expect(rounded).not.toMatch(/class="text"[^>]*rx=/)
    expect(rounded).not.toMatch(/edgeLabel[\s\S]*rx="/)
  })

  it('rounds flowchart decision diamonds into quadratic paths', () => {
    const raw = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120">
  <g class="node"><polygon class="label-container" points="60,10 110,60 60,110 10,60"/></g>
</svg>`
    const rounded = roundMermaidSvgRects(raw)
    expect(rounded.toLowerCase()).not.toContain('<polygon')
    expect(rounded).toMatch(/<path[^>]*d="M/)
    expect(rounded).toContain('Q')
    expect(rounded).toContain('label-container')
  })
})
