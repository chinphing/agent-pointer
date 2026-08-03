import { ref, watch, type Ref } from 'vue'

const STORAGE_KEY = 'pointer.sidebar.collapsed'

function readStored(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

function writeStored(collapsed: boolean) {
  try {
    localStorage.setItem(STORAGE_KEY, collapsed ? '1' : '0')
  } catch {
    /* ignore quota / private mode */
  }
}

// Module singleton so remount / multiple callers share one persisted value.
const collapsed: Ref<boolean> = ref(readStored())
let persistWired = false

function ensurePersistWatch() {
  if (persistWired) return
  persistWired = true
  watch(collapsed, writeStored, { flush: 'sync' })
}

/** Persisted left sidebar collapsed state (desktop). Restored on next open. */
export function useSidebarCollapse() {
  ensurePersistWatch()

  function toggle() {
    collapsed.value = !collapsed.value
  }

  return { collapsed, toggle }
}
