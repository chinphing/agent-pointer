/** Live-tool sweep: ~47px/s, never longer than 6s. */
export const TOOL_LIVE_SWEEP_MAX_SEC = 6
export const TOOL_LIVE_SWEEP_MIN_SEC = 2.1
export const TOOL_LIVE_SWEEP_PX_PER_SEC = 70 / 1.5

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
