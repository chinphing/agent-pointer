import { describe, expect, it } from 'vitest'
import {
  TOOL_LIVE_SWEEP_MAX_SEC,
  TOOL_LIVE_SWEEP_MIN_SEC,
  TOOL_LIVE_SWEEP_PX_PER_SEC,
  toolLiveSweepDuration,
  toolLiveSweepDurationSec,
} from './toolLiveSweep'

describe('toolLiveSweepDurationSec', () => {
  it('uses the max when width is missing', () => {
    expect(toolLiveSweepDurationSec(0)).toBe(TOOL_LIVE_SWEEP_MAX_SEC)
    expect(toolLiveSweepDurationSec(-4)).toBe(TOOL_LIVE_SWEEP_MAX_SEC)
  })

  it('scales with visible width and clamps to min/max', () => {
    // 170px and 255px are whole multiples of the px/s rate, so the formula
    // shape is pinned independently of the constant values.
    expect(toolLiveSweepDurationSec(TOOL_LIVE_SWEEP_PX_PER_SEC * 2)).toBe(2)
    expect(toolLiveSweepDurationSec(TOOL_LIVE_SWEEP_PX_PER_SEC * 3)).toBe(3)
    expect(toolLiveSweepDurationSec(800)).toBe(TOOL_LIVE_SWEEP_MAX_SEC)
    expect(toolLiveSweepDurationSec(40)).toBe(TOOL_LIVE_SWEEP_MIN_SEC)
  })

  it('formats CSS seconds', () => {
    expect(toolLiveSweepDuration(TOOL_LIVE_SWEEP_PX_PER_SEC * 2)).toBe('2.00s')
    expect(toolLiveSweepDuration(800)).toBe(`${TOOL_LIVE_SWEEP_MAX_SEC.toFixed(2)}s`)
  })
})
