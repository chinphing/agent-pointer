// @vitest-environment happy-dom

import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../types/chat'
import {
  CONVERSATION_NAV_PREVIEW_CHARS,
  CONVERSATION_NAV_TICK_GAP_PX,
  CONVERSATION_NAV_TICK_SLOT_PX,
  conversationNavFisheye,
  conversationNavFocusFromPointer,
  conversationNavPageDelta,
  conversationNavPreview,
  conversationNavRestTick,
  conversationNavScrollAffordances,
  conversationNavTranscriptScroller,
  conversationNavVisibleMessageId,
  mergeConversationNavItems
} from './conversationNav'

const user = (id: string, content: string): ChatMessage => ({
  id,
  role: 'user',
  content,
  status: 'done',
  createdAt: 1
})

describe('conversationNavPreview', () => {
  it('collapses whitespace and truncates to 36 chars', () => {
    expect(conversationNavPreview('hello\n\nworld')).toBe('hello world')
    expect(conversationNavPreview('   ')).toBe('（无文字）')
    const long = 'abcdefghijklmnopqrstuvwxyz0123456789EXTRA'
    const preview = conversationNavPreview(long)
    expect([...preview].length).toBe(CONVERSATION_NAV_PREVIEW_CHARS)
    expect(preview.endsWith('…')).toBe(true)
  })
})

describe('mergeConversationNavItems', () => {
  it('appends in-memory user turns that the API has not listed yet', () => {
    const merged = mergeConversationNavItems(
      [{ messageId: 'u1', preview: 'one' }],
      [
        user('u1', 'one'),
        {
          id: 'a-skip',
          role: 'assistant',
          content: 'x',
          status: 'done',
          createdAt: 1
        },
        user('u-syn', '[CUR_SCREEN] shot'),
        user('u-new', 'just sent')
      ]
    )
    expect(merged.map(i => i.messageId)).toEqual(['u1', 'u-new'])
    expect(merged[1]?.preview).toBe('just sent')
  })
})

describe('conversationNavFisheye', () => {
  it('peaks at the pointer and falls off with distance', () => {
    const center = conversationNavFisheye(0)
    const near = conversationNavFisheye(1)
    const far = conversationNavFisheye(6)
    expect(center.widthPx).toBeGreaterThan(near.widthPx)
    expect(near.widthPx).toBeGreaterThan(far.widthPx)
    expect(center.opacity).toBeGreaterThan(near.opacity)
    expect(far.widthPx).toBe(conversationNavFisheye(10).widthPx)
    expect(conversationNavRestTick(true).widthPx).toBe(6)
    expect(conversationNavRestTick(false).widthPx).toBe(5)
    expect(conversationNavRestTick(false).heightPx).toBe(1.5)
  })
})

describe('conversationNavTranscriptScroller', () => {
  it('finds the transcript scroller beside the nav overlay', () => {
    const root = document.createElement('div')
    const list = document.createElement('div')
    const scroller = document.createElement('div')
    scroller.className = 'chat-scroll-area'
    list.append(scroller)
    const aside = document.createElement('aside')
    root.append(list, aside)
    expect(conversationNavTranscriptScroller(aside)).toBe(scroller)
    expect(conversationNavTranscriptScroller(null)).toBeNull()
  })
})

describe('conversationNavFocusFromPointer', () => {
  const stride = CONVERSATION_NAV_TICK_SLOT_PX + CONVERSATION_NAV_TICK_GAP_PX
  const firstTop = 40

  it('peaks on the tick whose slot contains the pointer', () => {
    expect(
      conversationNavFocusFromPointer({
        pointerY: firstTop + CONVERSATION_NAV_TICK_SLOT_PX / 2,
        firstTickTop: firstTop,
        tickCount: 10
      })
    ).toBe(0)
    expect(
      conversationNavFocusFromPointer({
        pointerY: firstTop + stride + CONVERSATION_NAV_TICK_SLOT_PX / 2,
        firstTickTop: firstTop,
        tickCount: 10
      })
    ).toBe(1)
    expect(
      conversationNavFocusFromPointer({
        pointerY: firstTop + 9 * stride + CONVERSATION_NAV_TICK_SLOT_PX / 2,
        firstTickTop: firstTop,
        tickCount: 10
      })
    ).toBe(9)
  })

  it('clamps to the first and last tick', () => {
    expect(
      conversationNavFocusFromPointer({
        pointerY: firstTop - 20,
        firstTickTop: firstTop,
        tickCount: 10
      })
    ).toBe(0)
    expect(
      conversationNavFocusFromPointer({
        pointerY: firstTop + 40 * stride,
        firstTickTop: firstTop,
        tickCount: 10
      })
    ).toBe(9)
  })
})

describe('conversationNavScrollAffordances', () => {
  it('hides arrows when the first and last ticks sit inside the rail', () => {
    expect(
      conversationNavScrollAffordances({
        firstTickTop: 12,
        lastTickBottom: 200,
        viewportTop: 8,
        viewportBottom: 208
      })
    ).toEqual({ up: false, down: false })
  })

  it('ignores leftover padding below the last tick', () => {
    expect(
      conversationNavScrollAffordances({
        firstTickTop: 10,
        lastTickBottom: 204,
        viewportTop: 8,
        viewportBottom: 208
      }).down
    ).toBe(false)
  })

  it('shows down when the last tick is clipped', () => {
    expect(
      conversationNavScrollAffordances({
        firstTickTop: 10,
        lastTickBottom: 220,
        viewportTop: 8,
        viewportBottom: 208
      })
    ).toEqual({ up: false, down: true })
  })

  it('shows up when the first tick is clipped', () => {
    expect(
      conversationNavScrollAffordances({
        firstTickTop: 0,
        lastTickBottom: 200,
        viewportTop: 8,
        viewportBottom: 208
      })
    ).toEqual({ up: true, down: false })
  })
})

describe('conversationNavPageDelta', () => {
  it('moves about one visible tick window, with a floor', () => {
    expect(conversationNavPageDelta(200)).toBe(170)
    expect(conversationNavPageDelta(20)).toBe(48)
  })
})

describe('conversationNavVisibleMessageId', () => {
  it('pins to the last loaded turn when the transcript is at the bottom', () => {
    expect(
      conversationNavVisibleMessageId({
        atBottom: true,
        lastLoadedTurnId: 'turn-8',
        markerTurnId: 'turn-2'
      })
    ).toBe('turn-8')
  })

  it('keeps the viewport marker while the user is reading mid-thread', () => {
    expect(
      conversationNavVisibleMessageId({
        atBottom: false,
        lastLoadedTurnId: 'turn-8',
        markerTurnId: 'turn-2'
      })
    ).toBe('turn-2')
  })
})
