import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

/**
 * The transcript's free space is flex space, never a percentage of the scroller
 * height.
 *
 * `MessageList.vue` is only reachable through the app shell — no fixture mounts it
 * — so, like `lib/messageVirtualization.rowContainment.test.ts`, this guards the
 * markup that carries the contract.
 *
 * Why it matters: a percentage `min-height` on the transcript wrapper resolves
 * against the scroller's content-box height, so **every** scroller resize (composer
 * growth, window resize) invalidates the whole transcript subtree and re-runs its
 * layout. A non-shrinking flex item is sized by its own content instead, so a
 * resize only moves the box.
 *
 * The counterpart behaviour — short conversations start at the top, the blank stays
 * between the last turn and the composer — is deliberate (reverted from an elastic
 * blank in `ac73b132`, see `docs/zh-CN/ui/message-list-scroll-follow.md` §5), and
 * is asserted below so the elastic blank cannot come back unnoticed.
 */
const source = readFileSync(new URL('./MessageList.vue', import.meta.url), 'utf8')

/** Comments are prose about the markup: strip them so a reworded comment cannot fail a markup contract. */
const markup = source.replace(/<!--[\s\S]*?-->/g, '')

const SCROLLER = 'ref="scroller"'
const CONTENT = 'class="chat-column relative w-full shrink-0"'

function indexOf(marker: string): number {
  const at = markup.indexOf(marker)
  expect(at, `expected ${marker} in MessageList.vue`).toBeGreaterThan(-1)
  return at
}

/** The whole start tag of the element whose markup contains `marker`. */
function startTagOf(marker: string): string {
  const at = indexOf(marker)
  const end = markup.indexOf('>', at)
  expect(end).toBeGreaterThan(at)
  return markup.slice(at, end)
}

/**
 * The transcript wrapper: the first element opened after the scroller's start tag.
 * Found positionally rather than by its class list so reordering utilities is not a
 * test failure.
 */
function transcriptWrapperTag(): string {
  const scrollerEnd = markup.indexOf('>', indexOf(SCROLLER))
  const wrapperAt = markup.indexOf('<div', scrollerEnd)
  expect(wrapperAt, 'expected an element inside the scroller').toBeGreaterThan(scrollerEnd)
  const wrapperEnd = markup.indexOf('>', wrapperAt)
  expect(wrapperEnd).toBeGreaterThan(wrapperAt)
  return markup.slice(wrapperAt, wrapperEnd)
}

/** Everything from the scroller element to the virtualized content box. */
function freeSpaceRegion(): string {
  return markup.slice(indexOf(SCROLLER), indexOf(CONTENT))
}

describe('transcript free space', () => {
  it('makes the scroller a flex column, so free space is flex space', () => {
    const tag = startTagOf(SCROLLER)
    expect(tag).toContain('display: flex')
    expect(tag).toContain('flex-direction: column')
    // Still the scroll container every viewport read goes through.
    expect(tag).toContain('h-full')
    expect(tag).toContain('overflow-y-auto')
    expect(tag).toContain('chat-scroll-area')
  })

  it('sizes the transcript wrapper by its own content, not by the scroller height', () => {
    const tag = transcriptWrapperTag()
    expect(tag).not.toContain('min-h-full')
    expect(tag).not.toContain('height: 100%')
    // `flex-none` = `flex: 0 0 auto`: shrink 0 keeps the scroller's height out of the
    // wrapper's used size, grow 0 keeps it out of the free-space distribution.
    expect(tag).toContain('flex-none')
  })

  it('keeps the region between the scroller and the content box free of percentage heights', () => {
    const region = freeSpaceRegion()
    expect(region).not.toContain('min-h-full')
    expect(region).not.toContain('height: 100%')
    // The elastic blank that used to sit here (`min-h-0 flex-1`) was reverted.
    expect(region).not.toMatch(/\bflex-1\b/)
  })

  it('does not push a short conversation to the bottom', () => {
    // Reverted on purpose: the blank belongs between the last turn and the composer,
    // not above the first turn. An auto top margin (or a flex-end scroller) would
    // silently restore the old "new conversation looks pinned to the bottom" look.
    expect(transcriptWrapperTag()).not.toMatch(/margin-top:\s*auto|\bmt-auto\b/)
    expect(startTagOf(SCROLLER)).not.toMatch(/justify-content:\s*flex-end|\bjustify-end\b/)
  })

  it('keeps the virtualizer geometry contract on the content box', () => {
    const tag = startTagOf(CONTENT)
    // The rows are absolutely positioned inside a px-high box; letting it shrink
    // would let the scroller height rewrite the measured offsets.
    expect(tag).toContain('shrink-0')
    const after = markup.slice(indexOf(CONTENT), indexOf(CONTENT) + 200)
    expect(after).toContain('rowVirtualizer.getTotalSize()')
  })

  it('keeps the pull-to-load spacer first inside the wrapper', () => {
    // The "no more messages" hint and the top pull both live in that spacer, so it
    // has to stay above the sticky board and the content box.
    const wrapperAt = indexOf('class="flex w-full flex-none flex-col"')
    const pullAt = markup.indexOf('noOlderPullPx', wrapperAt)
    expect(pullAt).toBeGreaterThan(wrapperAt)
    expect(pullAt).toBeLessThan(indexOf(CONTENT))
  })
})
