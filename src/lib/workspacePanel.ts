import { t } from '../i18n'

export function gitInitializationTask(): string {
  return t('lib.gitInitTask')
}

/** @deprecated Prefer gitInitializationTask() so copy follows the active locale. */
export const GIT_INITIALIZATION_TASK = gitInitializationTask()
