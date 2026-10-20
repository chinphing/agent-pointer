// @vitest-environment node
//
// The composer textarea is a same-origin frame. Deleting the last character
// makes WebKit search backwards for an input-method word boundary; an empty
// in-page textarea has no text node, so that search walks the transcript.
// Pin the frame wiring without mounting Composer.
import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(new URL(path, import.meta.url), 'utf8')
}

describe('composer textarea slot', () => {
  const composer = source('./Composer.vue')
  const css = source('../../styles/globals.css')
  const frame = source('../../lib/composerTextareaFrame.ts')

  it('hosts the textarea in a same-origin frame inside the slot', () => {
    const slotAt = composer.indexOf('class="composer-textarea-slot')
    const frameAt = composer.indexOf('<iframe', slotAt)
    expect(slotAt).toBeGreaterThan(-1)
    expect(frameAt).toBeGreaterThan(slotAt)
    expect(composer).toContain('src="about:blank"')
    expect(composer).toContain('openComposerTextareaFrame')
    expect(composer).not.toContain('<textarea')
    expect(composer).toContain(":style=\"{ height: `${composerSlotHeightPx}px` }\"")
  })

  it('fills the slot with the frame and keeps the dock contained', () => {
    expect(css).toMatch(/\.composer-textarea-slot\s*\{[^}]*contain:\s*layout/)
    expect(css).toMatch(
      /\.composer-textarea-slot > \.composer-textarea-frame\s*\{[^}]*position:\s*absolute/
    )
    expect(composer).toContain('composer-dock')
    expect(css).toMatch(/\.composer-dock\s*\{[^}]*contain:\s*layout/)
  })

  it('gives the frame document a definite textarea height', () => {
    expect(frame).toContain('height: 36px')
    expect(frame).toContain('field-sizing: content')
    expect(frame).toContain('max-height: 250px')
  })
})
