// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { writeClipboardText } from './clipboardText'

/**
 * Own-property overrides for the two globals this module probes. happy-dom ships a
 * working `navigator.clipboard`, so each test states the state it needs instead of
 * inheriting one; both properties are removed again afterwards.
 */
function stubClipboard(clipboard: unknown): void {
  Object.defineProperty(navigator, 'clipboard', {
    value: clipboard,
    configurable: true,
    writable: true
  })
}

function stubExecCommand(execCommand: unknown): void {
  Object.defineProperty(document, 'execCommand', {
    value: execCommand,
    configurable: true,
    writable: true
  })
}

function textareas(): HTMLTextAreaElement[] {
  return [...document.querySelectorAll('textarea')]
}

beforeEach(() => {
  // Nothing is available unless a test installs it.
  stubClipboard(undefined)
  stubExecCommand(undefined)
})

afterEach(() => {
  Reflect.deleteProperty(navigator as unknown as Record<string, unknown>, 'clipboard')
  Reflect.deleteProperty(document as unknown as Record<string, unknown>, 'execCommand')
  for (const textarea of textareas()) textarea.remove()
  vi.restoreAllMocks()
})

describe('writeClipboardText', () => {
  it('uses the async Clipboard API when it is available', async () => {
    const writeText = vi.fn(async () => {})
    stubClipboard({ writeText })

    await expect(writeClipboardText('render perf\nfps 60')).resolves.toBe(true)

    expect(writeText).toHaveBeenCalledTimes(1)
    expect(writeText).toHaveBeenCalledWith('render perf\nfps 60')
    // The fallback must not have run as well.
    expect(textareas()).toEqual([])
  })

  it('falls back to a temporary textarea when the primary API rejects', async () => {
    const writeText = vi.fn(async () => {
      throw new Error('Document is not focused')
    })
    stubClipboard({ writeText })
    const execCommand = vi.fn(() => true)
    stubExecCommand(execCommand)

    await expect(writeClipboardText('the full HUD text')).resolves.toBe(true)

    expect(writeText).toHaveBeenCalledTimes(1)
    expect(execCommand).toHaveBeenCalledWith('copy')
    // The fallback copies what it was given, and leaves nothing behind.
    expect(textareas()).toEqual([])
  })

  it('falls back when the API is missing altogether (insecure context)', async () => {
    const execCommand = vi.fn(() => true)
    stubExecCommand(execCommand)

    await expect(writeClipboardText('text')).resolves.toBe(true)

    expect(execCommand).toHaveBeenCalledWith('copy')
  })

  it('copies the exact text through the textarea, and removes it again', async () => {
    stubExecCommand(vi.fn(() => true))
    let copied = ''

    const appendChild = document.body.appendChild.bind(document.body)
    vi.spyOn(document.body, 'appendChild').mockImplementation((node: Node) => {
      if (node instanceof HTMLTextAreaElement) copied = node.value
      return appendChild(node)
    })

    const payload = 'render perf  Cmd/Ctrl+Shift+Alt+P\nv.slack  124 px  pk  980 px'
    await writeClipboardText(payload)

    expect(copied).toBe(payload)
    expect(textareas()).toEqual([])
  })

  it('reports failure instead of throwing when both paths fail', async () => {
    stubClipboard({
      writeText: async () => {
        throw new Error('denied')
      }
    })
    stubExecCommand(vi.fn(() => false))

    await expect(writeClipboardText('text')).resolves.toBe(false)
    expect(textareas()).toEqual([])
  })

  it('reports failure when the fallback is unavailable too, and never throws', async () => {
    // No clipboard, no execCommand: nothing can copy, but nothing may throw.
    await expect(writeClipboardText('text')).resolves.toBe(false)

    stubClipboard({
      writeText: () => {
        throw new Error('denied')
      }
    })
    stubExecCommand(undefined)
    await expect(writeClipboardText('text')).resolves.toBe(false)
  })

  it('reports failure when the fallback itself throws', async () => {
    stubExecCommand(
      vi.fn(() => {
        throw new Error('copy is not allowed')
      })
    )

    await expect(writeClipboardText('text')).resolves.toBe(false)
    expect(textareas()).toEqual([])
  })
})
