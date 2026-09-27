import { t } from '../i18n'
/** Matches backend `task_board::planner::PLANNER_PHASE_THOUGHTS`. */
export const PLANNER_PHASE_THOUGHTS = t('planner.phaseThoughts')

export function isPlannerPhaseThoughts(thoughts: string | undefined | null): boolean {
  return thoughts?.trim() === PLANNER_PHASE_THOUGHTS
}
