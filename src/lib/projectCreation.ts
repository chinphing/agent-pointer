import type { ProjectCreationResult } from '../types/chat'

interface ProjectCreationActions {
  refreshProjects: () => Promise<void>
  selectProject: (projectId: string) => Promise<unknown>
  notify: (message: string) => void
}

/** Derive a project label from a macOS, Linux, or Windows workspace path. */
export function projectNameFromWorkspaceRoot(workspaceRoot: string): string {
  const normalized = workspaceRoot.trim().replace(/[\\/]+$/, '')
  const segments = normalized.split(/[\\/]/).filter(Boolean)
  return segments[segments.length - 1] ?? '新项目'
}

/** Keep project creation behavior identical for desktop IPC and web HTTP results. */
export async function applyProjectCreationResult(
  result: ProjectCreationResult,
  actions: ProjectCreationActions
): Promise<void> {
  await actions.refreshProjects()
  await actions.selectProject(result.project.id)
  if (!result.reusedExisting) return
  actions.notify(`目录已关联到现有项目「${result.project.name}」`)
}
