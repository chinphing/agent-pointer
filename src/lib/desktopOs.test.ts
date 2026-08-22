import { describe, expect, it } from 'vitest'
import { detectDesktopOs, macTrafficLightInsetActive, type DesktopOs } from './desktopOs'

describe('detectDesktopOs', () => {
  it('detects macOS from userAgent', () => {
    expect(detectDesktopOs({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)', platform: '' })).toBe('macos')
  })

  it('detects Windows from userAgent', () => {
    expect(detectDesktopOs({ userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)', platform: '' })).toBe('windows')
  })

  it('detects Linux from userAgent', () => {
    expect(detectDesktopOs({ userAgent: 'Mozilla/5.0 (X11; Linux x86_64)', platform: '' })).toBe('linux')
  })
})

describe('macTrafficLightInsetActive', () => {
  const cases: Array<[DesktopOs, boolean, boolean]> = [
    ['macos', false, true],
    ['macos', true, false],
    ['windows', false, false],
    ['windows', true, false],
    ['linux', false, false],
    ['linux', true, false],
    ['unknown', false, false],
    ['unknown', true, false]
  ]

  it.each(cases)('os=%s fullscreen=%s → %s', (os, fullscreen, expected) => {
    expect(macTrafficLightInsetActive(os, fullscreen)).toBe(expected)
  })
})
