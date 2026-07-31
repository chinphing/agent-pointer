import { describe, expect, it } from 'vitest'
import {
  shouldShowMessageListPlaceholder,
  shouldShowWelcomeHome
} from './chatMainPane'

describe('chat main pane state', () => {
  it('keeps a loading surface during boot before a conversation is selected', () => {
    const placeholder = shouldShowMessageListPlaceholder(false, false, false)
    expect(placeholder).toBe(true)
    expect(shouldShowWelcomeHome(false, 0, placeholder)).toBe(false)
  })

  it('shows the welcome screen only for a selected, empty conversation', () => {
    const placeholder = shouldShowMessageListPlaceholder(true, false, false)
    expect(placeholder).toBe(false)
    expect(shouldShowWelcomeHome(true, 0, placeholder)).toBe(true)
    expect(shouldShowWelcomeHome(true, 1, placeholder)).toBe(false)
  })
})
