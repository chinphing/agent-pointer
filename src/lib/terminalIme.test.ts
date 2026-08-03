// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest'
import {
  createTerminalImeGuard,
  isAbandonedImeAsciiBuffer,
  shouldDeferKeyToIme
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

  it('does not swallow WKWebView keyCode 0 for Esc / vim keys', () => {
    expect(shouldDeferKeyToIme({ isComposing: false, keyCode: 0, key: 'Escape' } as KeyboardEvent)).toBe(false)
    expect(shouldDeferKeyToIme({ isComposing: false, keyCode: 0, key: 'h' } as KeyboardEvent)).toBe(false)
    expect(shouldDeferKeyToIme({ isComposing: false, keyCode: 0, key: 'ArrowUp' } as KeyboardEvent)).toBe(false)
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

  it('cancels composition fallback when an escape sequence arrives', async () => {
    vi.useFakeTimers()
    const send = vi.fn()
    const guard = createTerminalImeGuard(send)
    const ta = document.createElement('textarea')
    guard.attach(ta)

    const end = new CompositionEvent('compositionend')
    Object.defineProperty(end, 'data', { value: 'ni' })
    ta.dispatchEvent(end)
    expect(guard.filterData('\x1b')).toBe('\x1b')
    await vi.advanceTimersByTimeAsync(100)
    expect(send).not.toHaveBeenCalled()

    guard.detach()
    vi.useRealTimers()
  })
})
