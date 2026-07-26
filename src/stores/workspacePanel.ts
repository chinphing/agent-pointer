import { defineStore } from 'pinia'
import { ref } from 'vue'

export type WorkspaceTurnDiffRequest = {
  conversationId: string
  turnId: string
  path: string
}

const OPEN_STORAGE_KEY = 'pointer.workspacePanel.open'

function readStoredOpen(): boolean {
  return typeof localStorage !== 'undefined' && localStorage.getItem(OPEN_STORAGE_KEY) === 'true'
}

export const useWorkspacePanelStore = defineStore('workspacePanel', () => {
  const open = ref(readStoredOpen())
  const pendingTurnDiff = ref<WorkspaceTurnDiffRequest | null>(null)

  function setOpen(next: boolean) {
    open.value = next
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(OPEN_STORAGE_KEY, String(next))
    }
  }

  function openTurnDiff(request: WorkspaceTurnDiffRequest) {
    pendingTurnDiff.value = request
    setOpen(true)
  }

  function consumePendingTurnDiff(): WorkspaceTurnDiffRequest | null {
    const next = pendingTurnDiff.value
    pendingTurnDiff.value = null
    return next
  }

  return {
    open,
    pendingTurnDiff,
    setOpen,
    openTurnDiff,
    consumePendingTurnDiff
  }
})
