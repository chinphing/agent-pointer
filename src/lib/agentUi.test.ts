import { describe, expect, it } from 'vitest'
import { mergeDebugDisplayUi, resolveAgentUi } from './agentUi'

describe('agent UI defaults', () => {
  it('shows tool-call results by default', () => {
    expect(resolveAgentUi(undefined).showToolCallResults).toBe(true)
  })

  it('does not alter display defaults when debug menus are enabled', () => {
    const normal = resolveAgentUi(undefined)
    expect(
      mergeDebugDisplayUi({ debugMenusEnabled: true, agentUiOverrides: {} }, 'general', normal)
    ).toEqual(normal)
  })
})
