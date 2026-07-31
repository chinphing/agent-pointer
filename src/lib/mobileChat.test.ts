import { describe, expect, it } from 'vitest'
import {
  MOBILE_NEW_CONVERSATION_TURN_THRESHOLD,
  shouldShowMobileNewConversationButton
} from './mobileChat'

describe('mobile new conversation entry', () => {
  it('appears only on mobile after more than 30 user-anchored turns', () => {
    expect(
      shouldShowMobileNewConversationButton(true, MOBILE_NEW_CONVERSATION_TURN_THRESHOLD)
    ).toBe(false)
    expect(
      shouldShowMobileNewConversationButton(true, MOBILE_NEW_CONVERSATION_TURN_THRESHOLD + 1)
    ).toBe(true)
    expect(
      shouldShowMobileNewConversationButton(false, MOBILE_NEW_CONVERSATION_TURN_THRESHOLD + 1)
    ).toBe(false)
  })
})
