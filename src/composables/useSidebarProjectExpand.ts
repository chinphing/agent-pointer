import { ref, watch, type Ref } from 'vue'

const STORAGE_KEY = 'pointer.sidebar.expandedProjectIds'

function readStored(): Set<string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return new Set()
    const parsed = JSON.parse(raw) as unknown
    if (!Array.isArray(parsed)) return new Set()
    return new Set(
      parsed.filter((id): id is string => typeof id === 'string' && id.trim().length > 0)
    )
  } catch {
    return new Set()
  }
}

function writeStored(ids: Set<string>) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify([...ids]))
  } catch {
    /* ignore quota / private mode */
  }
}

// Module singleton so AppShell remount / multiple callers share one persisted set.
const expandedProjectIds: Ref<Set<string>> = ref(readStored())
let persistWired = false

function ensurePersistWatch() {
  if (persistWired) return
  persistWired = true
  watch(expandedProjectIds, ids => writeStored(ids))
}

/** Persisted per-project expand state for the sidebar project list. */
export function useSidebarProjectExpand() {
  ensurePersistWatch()

  function isExpanded(projectId: string): boolean {
    return expandedProjectIds.value.has(projectId)
  }

  function setExpanded(projectId: string, expanded: boolean) {
    const id = projectId.trim()
    if (!id) return
    const next = new Set(expandedProjectIds.value)
    if (expanded) next.add(id)
    else next.delete(id)
    expandedProjectIds.value = next
  }

  function toggleExpanded(projectId: string): boolean {
    const nextExpanded = !isExpanded(projectId)
    setExpanded(projectId, nextExpanded)
    return nextExpanded
  }

  function forgetProject(projectId: string) {
    setExpanded(projectId, false)
  }

  return {
    expandedProjectIds,
    isExpanded,
    setExpanded,
    toggleExpanded,
    forgetProject
  }
}
