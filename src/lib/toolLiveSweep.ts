/**
 * Live-tool sweep: ~85px/s across the visible text box, clamped to 1.6–3.4s.
 *
 * The clip is 3.6em wide and its travel is `box + 3.6em` (see globals.css), so
 * the on-screen speed is a little above 85px/s — the extra band width is not
 * part of the measured box. Long labels hit the cap and sweep a bit faster;
 * short ones sit on the floor.
 */
export const TOOL_LIVE_SWEEP_MAX_SEC = 3.4
export const TOOL_LIVE_SWEEP_MIN_SEC = 1.6
export const TOOL_LIVE_SWEEP_PX_PER_SEC = 85

export function toolLiveSweepDurationSec(widthPx: number): number {
  if (!Number.isFinite(widthPx) || widthPx <= 0) return TOOL_LIVE_SWEEP_MAX_SEC
  return Math.min(
    TOOL_LIVE_SWEEP_MAX_SEC,
    Math.max(TOOL_LIVE_SWEEP_MIN_SEC, widthPx / TOOL_LIVE_SWEEP_PX_PER_SEC)
  )
}

export function toolLiveSweepDuration(widthPx: number): string {
  return `${toolLiveSweepDurationSec(widthPx).toFixed(2)}s`
}
