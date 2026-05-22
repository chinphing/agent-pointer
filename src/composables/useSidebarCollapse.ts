import { ref, watch } from 'vue'

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

/** Persisted left sidebar collapsed state (desktop). */
export function useSidebarCollapse() {
  const collapsed = ref(readStored())

  watch(collapsed, writeStored)

  function toggle() {
    collapsed.value = !collapsed.value
  }

  return { collapsed, toggle }
}
