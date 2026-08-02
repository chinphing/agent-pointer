import type { Project, SkillDef } from '../types/chat'

export interface SkillDirectoryCandidate {
  name: string
  path: string
}

/**
 * User skill directories (`~/.pointer/skills` / `~/.agents/skills`, i.e. anything
 * that is not a bundled `system` skill) that can be turned into a project.
 *
 * Already-existing project roots are excluded so the candidate list stays focused
 * on directories that still need a project.
 */
export function userSkillDirectoryCandidates(
  skills: readonly SkillDef[],
  existingProjects: readonly Project[]
): SkillDirectoryCandidate[] {
  const existingRoots = new Set(
    existingProjects
      .filter(p => !p.isArchived)
      .map(p => p.workspaceRoot?.trim())
      .filter(Boolean)
  )
  const seen = new Set<string>()
  const out: SkillDirectoryCandidate[] = []
  for (const skill of skills) {
    const path = skill.source?.trim()
    if (!path || skill.provenance === 'system' || existingRoots.has(path)) {
      continue
    }
    out.push({ name: skill.name, path })
  }
  out.sort((a, b) => a.name.localeCompare(b.name))
  // Same directory may map to several skill names; keep the first after sorting.
  return out.filter(candidate => {
    if (seen.has(candidate.path)) return false
    seen.add(candidate.path)
    return true
  })
}
