import { ref, watch, type Ref } from 'vue'

const STORAGE_KEY = 'pointer.sidebar.sectionCollapse'

export type SidebarSectionCollapseState = {
  pinned: boolean
  projects: boolean
  conversations: boolean
}

const DEFAULT_STATE: SidebarSectionCollapseState = {
  pinned: false,
  projects: false,
  conversations: false
}

function readStored(): SidebarSectionCollapseState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULT_STATE }
    const parsed = JSON.parse(raw) as Partial<SidebarSectionCollapseState>
    return {
      pinned: parsed.pinned === true,
      projects: parsed.projects === true,
      conversations: parsed.conversations === true
    }
  } catch {
    return { ...DEFAULT_STATE }
  }
}

function writeStored(state: SidebarSectionCollapseState) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {
    /* ignore quota / private mode */
  }
}

const sectionCollapse: Ref<SidebarSectionCollapseState> = ref(readStored())
let persistWired = false

function ensurePersistWatch() {
  if (persistWired) return
  persistWired = true
  // Replace whole object on patch; sync flush keeps localStorage ordered with toggles.
  watch(sectionCollapse, state => writeStored(state), { flush: 'sync' })
}

function patch(partial: Partial<SidebarSectionCollapseState>) {
  sectionCollapse.value = { ...sectionCollapse.value, ...partial }
}

/**
 * Persisted collapse state for sidebar sections (Pinned / Projects / Recent).
 * Restored from localStorage on next open (app + web).
 */
export function useSidebarSectionCollapse() {
  ensurePersistWatch()

  function togglePinned() {
    patch({ pinned: !sectionCollapse.value.pinned })
  }

  function toggleProjects() {
    patch({ projects: !sectionCollapse.value.projects })
  }

  function toggleConversations() {
    patch({ conversations: !sectionCollapse.value.conversations })
  }

  function expandProjects() {
    if (sectionCollapse.value.projects) patch({ projects: false })
  }

  function expandConversations() {
    if (sectionCollapse.value.conversations) patch({ conversations: false })
  }

  return {
    sectionCollapse,
    togglePinned,
    toggleProjects,
    toggleConversations,
    expandProjects,
    expandConversations
  }
}

/** Test helper: reload module state from localStorage (after clear/seed). */
export function __resetSidebarSectionCollapseForTests() {
  sectionCollapse.value = readStored()
}
