import { describe, expect, it } from 'vitest'
import { scrollTopForAnchor } from './messageListScrollAnchor'

describe('messageListScrollAnchor', () => {
  it('scrollTopForAnchor keeps the turn at the captured viewport offset', () => {
    // Turn starts at 4000 in the content; it was 120px below the scroller top.
    expect(scrollTopForAnchor(4000, 120)).toBe(3880)
  })

  it('scrollTopForAnchor never goes negative', () => {
    expect(scrollTopForAnchor(50, 120)).toBe(0)
  })
})
