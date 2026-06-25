/** Matches backend `task_board::planner::PLANNER_PHASE_THOUGHTS`. */
export const PLANNER_PHASE_THOUGHTS = '正在规划任务…'

export function isPlannerPhaseThoughts(thoughts: string | undefined | null): boolean {
  return thoughts?.trim() === PLANNER_PHASE_THOUGHTS
}
