import { describe, expect, it } from 'vitest'
import postcss from 'postcss'
import legacyCssCompat from './postcss-legacy-compat.mjs'

async function run(css) {
  const result = await postcss([legacyCssCompat()]).process(css, { from: undefined })
  return result.css.replace(/\s+/g, ' ').trim()
}

describe(':is() / :where() expansion', () => {
  it('expands :is() into a plain selector list', async () => {
    const out = await run(':is(.a, .b) svg { color: red }')
    expect(out).toContain('.a svg, .b svg { color: red }')
  })

  it('expands :where() the same way', async () => {
    const out = await run(':where(.x, .y) { color: red }')
    expect(out).toContain('.x, .y { color: red }')
  })

  it('expands nested :is() as a cartesian product', async () => {
    const out = await run(':is(.a, .b) :is(rect, path) { fill: red }')
    expect(out).toContain('.a rect, .a path, .b rect, .b path { fill: red }')
  })

  it('leaves a single-argument :is() as the bare selector', async () => {
    const out = await run(':is(.only) { color: red }')
    expect(out).toContain('.only { color: red }')
  })

  it('does NOT expand :is() nested inside another functional pseudo', async () => {
    // `:not(:is(a, b))` means "neither a nor b"; expanding would mean "not a, or not b".
    const out = await run('.card:not(:is(.a, .b)) { color: red }')
    expect(out).toContain('.card:not(:is(.a, .b)) { color: red }')
    expect(out).not.toContain('.card:not(.a)')
  })

  it('keeps the original rule alongside the legacy clone', async () => {
    const out = await run(':is(.a, .b) { color: red }')
    expect(out).toContain(':is(.a, .b) { color: red }')
    expect(out).toContain('.a, .b { color: red }')
  })

  it('leaves an already-safe selector untouched', async () => {
    const out = await run('.plain > span { color: red }')
    expect(out).toBe('.plain > span { color: red }')
  })
})

describe(':has() isolation', () => {
  it('drops the :has() entry but keeps the rest of the list', async () => {
    const out = await run('.a:has(img), .b { color: red }')
    expect(out).toContain('.b { color: red }')
    // The original rule survives for modern browsers.
    expect(out).toContain('.a:has(img), .b { color: red }')
  })

  it('emits nothing extra when every selector needs :has()', async () => {
    const out = await run('.a:has(img), .b:has(svg) { color: red }')
    expect(out).toBe('.a:has(img), .b:has(svg) { color: red }')
  })

  it('still isolates :focus-visible the way it did before', async () => {
    const out = await run('.btn:focus-visible, .btn:hover { color: red }')
    expect(out).toContain('.btn:hover { color: red }')
  })
})

describe('flex gap fallback', () => {
  it('expands :is() in the generated margin rules', async () => {
    const out = await run(':is(.row, .col) { display: flex; gap: 8px }')
    expect(out).toContain('html.no-flex-gap .row > svg')
    expect(out).toContain('html.no-flex-gap .col > svg')
  })

  it('skips the fallback when the selector needs :has()', async () => {
    const out = await run('.card:has(img) { display: flex; gap: 8px }')
    expect(out).not.toContain('no-flex-gap')
  })
})
