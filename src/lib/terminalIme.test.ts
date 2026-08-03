// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest'
import {
  createTerminalImeGuard,
  isAbandonedImeAsciiBuffer,
  isWebKitTerminalHost,
  shouldDeferKeyToIme,
  terminalFontFamily,
  TERMINAL_CJK_FONT_FAMILY_CHROMIUM,
  TERMINAL_CJK_FONT_FAMILY_WEBKIT
} from './terminalIme'

describe('shouldDeferKeyToIme', () => {
  it('defers composing and IME process keys', () => {
    expect(shouldDeferKeyToIme({ isComposing: true, keyCode: 65, key: 'a' } as KeyboardEvent)).toBe(true)
    expect(shouldDeferKeyToIme({ isComposing: false, keyCode: 229, key: 'a' } as KeyboardEvent)).toBe(true)
    expect(shouldDeferKeyToIme({ isComposing: false, keyCode: 0, key: 'Process' } as KeyboardEvent)).toBe(true)
    expect(shouldDeferKeyToIme({ isComposing: false, keyCode: 65, key: 'Dead' } as KeyboardEvent)).toBe(true)
  })

  it('keeps normal ASCII keys for xterm', () => {
    expect(shouldDeferKeyToIme({ isComposing: false, keyCode: 65, key: 'a' } as KeyboardEvent)).toBe(false)
    expect(shouldDeferKeyToIme({ isComposing: false, keyCode: 13, key: 'Enter' } as KeyboardEvent)).toBe(false)
  })
})

describe('isAbandonedImeAsciiBuffer', () => {
  it('detects spaced pinyin buffers only', () => {
    expect(isAbandonedImeAsciiBuffer('ni hao')).toBe(true)
    expect(isAbandonedImeAsciiBuffer('nihao')).toBe(false)
    expect(isAbandonedImeAsciiBuffer('你好')).toBe(false)
    expect(isAbandonedImeAsciiBuffer('   ')).toBe(false)
  })
})

describe('terminalFontFamily', () => {
  it('puts CJK faces before Latin monospace on WebKit hosts', () => {
    const family = terminalFontFamily()
    if (isWebKitTerminalHost()) {
      expect(family).toBe(TERMINAL_CJK_FONT_FAMILY_WEBKIT)
      expect(family.indexOf('PingFang')).toBeLessThan(family.indexOf('Menlo'))
    } else {
      expect(family).toBe(TERMINAL_CJK_FONT_FAMILY_CHROMIUM)
      expect(family.indexOf('Menlo')).toBeLessThan(family.indexOf('PingFang'))
    }
  })
})

describe('createTerminalImeGuard', () => {
  it('forwards insertText that xterm would drop while a key is down', () => {
    const send = vi.fn()
    const guard = createTerminalImeGuard(send)
    const ta = document.createElement('textarea')
    guard.attach(ta)

    guard.observeKeyEvent({ type: 'keydown', keyCode: 16 } as KeyboardEvent)
    ta.dispatchEvent(
      new InputEvent('input', { bubbles: true, inputType: 'insertText', data: '？', composed: true })
    )

    expect(send).toHaveBeenCalledWith('？')
    guard.detach()
  })

  it('does not forward insertText after IME-consumed keydown 229', () => {
    const send = vi.fn()
    const guard = createTerminalImeGuard(send)
    const ta = document.createElement('textarea')
    guard.attach(ta)

    guard.observeKeyEvent({ type: 'keydown', keyCode: 229 } as KeyboardEvent)
    ta.dispatchEvent(
      new InputEvent('input', { bubbles: true, inputType: 'insertText', data: '中', composed: true })
    )

    expect(send).not.toHaveBeenCalled()
    guard.detach()
  })

  it('strips abandoned pinyin spaces when xterm delivers the buffer', () => {
    const send = vi.fn()
    const guard = createTerminalImeGuard(send)
    const ta = document.createElement('textarea')
    guard.attach(ta)

    // happy-dom may omit CompositionEventInit.data; set it explicitly.
    const end = new CompositionEvent('compositionend')
    Object.defineProperty(end, 'data', { value: 'ni hao' })
    ta.dispatchEvent(end)
    expect(guard.filterData('ni hao')).toBe('nihao')

    guard.detach()
  })
})
