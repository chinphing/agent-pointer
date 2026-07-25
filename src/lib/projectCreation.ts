import type { ProjectCreationResult } from '../types/chat'

interface ProjectCreationActions {
  refreshProjects: () => Promise<void>
  switchProject: (projectId: string) => Promise<unknown>
  notify: (message: string) => void
}

/** Keep project creation behavior identical for desktop IPC and web HTTP results. */
export async function applyProjectCreationResult(
  result: ProjectCreationResult,
  actions: ProjectCreationActions
): Promise<void> {
  await actions.refreshProjects()
  await actions.switchProject(result.project.id)
  if (!result.reusedExisting) return
  actions.notify(`目录已关联到现有项目「${result.project.name}」`)
}
