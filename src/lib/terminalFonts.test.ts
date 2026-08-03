import { describe, expect, it } from 'vitest'
import {
  TERMINAL_FONT_FAMILY_LINUX,
  TERMINAL_FONT_FAMILY_MAC,
  TERMINAL_FONT_FAMILY_WIN,
  terminalFontFamily
} from './terminalFonts'

describe('terminalFontFamily', () => {
  it('keeps system monospace before PingFang on macOS', () => {
    const stack = terminalFontFamily({ os: 'macos' })
    expect(stack).toBe(TERMINAL_FONT_FAMILY_MAC)
    expect(stack.startsWith('ui-monospace')).toBe(true)
    expect(stack.indexOf('Menlo')).toBeLessThan(stack.indexOf('PingFang SC'))
  })

  it('uses YaHei after Consolas on Windows', () => {
    const stack = terminalFontFamily({ os: 'windows' })
    expect(stack).toBe(TERMINAL_FONT_FAMILY_WIN)
    expect(stack.indexOf('Consolas')).toBeLessThan(stack.indexOf('Microsoft YaHei'))
  })

  it('uses distro CJK UI fonts after monospace on Linux', () => {
    const stack = terminalFontFamily({ os: 'linux' })
    expect(stack).toBe(TERMINAL_FONT_FAMILY_LINUX)
    expect(stack.startsWith('ui-monospace')).toBe(true)
  })
})
