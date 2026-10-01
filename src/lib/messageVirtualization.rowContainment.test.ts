import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { MESSAGE_VIRTUAL_ROW_CLASS } from './messageVirtualization'

// The row is only reachable through MessageList.vue (no component fixture), so
// these guard the two surfaces that actually carry the containment contract:
// the class rule in the stylesheet and the binding in the row template.
const globalsCss = readFileSync(new URL('../styles/globals.css', import.meta.url), 'utf8')
const messageListSource = readFileSync(
  new URL('../components/chat/MessageList.vue', import.meta.url),
  'utf8'
)

function cssRuleBody(source: string, className: string): string {
  const match = new RegExp(`\\n\\s*\\.${className}\\s*\\{([^}]*)\\}`).exec(source)
  expect(match, `expected a .${className} rule in globals.css`).toBeTruthy()
  return match?.[1] ?? ''
}

/**
 * The row wrapper carries the inline `translateY` that positions it. Slice its
 * start tag so the assertions below prove the class and the transform land on
 * the same element — a class on a parent would not scope that row's layout.
 */
function virtualRowStartTag(source: string): string {
  const loopAt = source.indexOf('v-for="row in renderedRows"')
  expect(loopAt).toBeGreaterThan(-1)
  const styleAt = source.indexOf(':style="{', loopAt)
  expect(styleAt).toBeGreaterThan(loopAt)
  const tagEnd = source.indexOf('>', styleAt)
  expect(tagEnd).toBeGreaterThan(styleAt)
  return source.slice(loopAt, tagEnd)
}

describe('virtual message row containment', () => {
  it('scopes each row layout without promoting a compositor layer per row', () => {
    const body = cssRuleBody(globalsCss, MESSAGE_VIRTUAL_ROW_CLASS)
    expect(body).toMatch(/contain:\s*layout\b/)
    // `will-change: transform` was reverted: promoting every rendered row
    // (20-30 tall rows) to its own composited layer left unpainted areas while
    // the compositor caught up during fast scrolling. The inline `translateY`
    // still animates on the compositor, so the hint bought nothing.
    expect(body).not.toMatch(/will-change/)
  })

  it('never clips the row or skips its subtree', () => {
    const body = cssRuleBody(globalsCss, MESSAGE_VIRTUAL_ROW_CLASS)
    // `contain: paint` would clip hover pills / avatar slots that hang outside
    // the row box; `content-visibility` would make the measure batch's
    // `offsetHeight` read wrong; size containment would collapse the row.
    expect(body).not.toMatch(/paint/)
    expect(body).not.toMatch(/content-visibility/)
    expect(body).not.toMatch(/\bsize\b/)
  })

  it('applies the class on the element that carries the translateY transform', () => {
    const rowTag = virtualRowStartTag(messageListSource)
    expect(rowTag).toContain('MESSAGE_VIRTUAL_ROW_CLASS')
    expect(rowTag).toContain('translateY(')
    expect(rowTag).toContain('row.virtualRow.start')
    expect(rowTag).not.toContain('contentVisibility')
  })
})
