import { describe, expect, it } from 'vitest'
import {
  TOOL_LIVE_SWEEP_MAX_SEC,
  TOOL_LIVE_SWEEP_MIN_SEC,
  toolLiveSweepDuration,
  toolLiveSweepDurationSec,
} from './toolLiveSweep'

describe('toolLiveSweepDurationSec', () => {
  it('uses 6s when width is missing', () => {
    expect(toolLiveSweepDurationSec(0)).toBe(TOOL_LIVE_SWEEP_MAX_SEC)
    expect(toolLiveSweepDurationSec(-4)).toBe(TOOL_LIVE_SWEEP_MAX_SEC)
  })

  it('scales with visible width and caps at 6s', () => {
    expect(toolLiveSweepDurationSec(140)).toBe(3)
    expect(toolLiveSweepDurationSec(280)).toBe(TOOL_LIVE_SWEEP_MAX_SEC)
    expect(toolLiveSweepDurationSec(800)).toBe(TOOL_LIVE_SWEEP_MAX_SEC)
    expect(toolLiveSweepDurationSec(40)).toBe(TOOL_LIVE_SWEEP_MIN_SEC)
  })

  it('formats CSS seconds', () => {
    expect(toolLiveSweepDuration(140)).toBe('3.00s')
    expect(toolLiveSweepDuration(280)).toBe('6.00s')
  })
})
