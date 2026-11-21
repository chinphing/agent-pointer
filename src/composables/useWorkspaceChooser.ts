import { ref } from 'vue'
import {
  createWorkspaceChooserDir,
  getWorkspacePolicy,
  listWorkspaceChooserDirs,
  selectWorkspacePath
} from '../lib/api'
import type {
  WorkspaceChooserEntry,
  WorkspaceListResult,
  WorkspacePolicy,
  WorkspaceSelectResult
} from '../types/chat'

/**
 * Shared workspace directory chooser for Client and Web.
 * Security comes from WorkspacePolicy returned by the backend.
 */
export function useWorkspaceChooser() {
  const policy = ref<WorkspacePolicy | null>(null)
  const listing = ref<WorkspaceListResult | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function loadPolicy() {
    policy.value = await getWorkspacePolicy()
    return policy.value
  }

  async function browse(parent?: string, conversationId?: string) {
    loading.value = true
    error.value = null
    try {
      listing.value = await listWorkspaceChooserDirs(parent, conversationId)
      return listing.value
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      throw e
    } finally {
      loading.value = false
    }
  }

  async function createDir(parent: string, name: string, conversationId?: string) {
    const entry = await createWorkspaceChooserDir(parent, name, conversationId)
    await browse(parent, conversationId)
    return entry
  }

  async function select(path: string, conversationId?: string): Promise<WorkspaceSelectResult> {
    return await selectWorkspacePath(path, conversationId)
  }

  return {
    policy,
    listing,
    loading,
    error,
    loadPolicy,
    browse,
    createDir,
    select
  }
}

export type { WorkspaceChooserEntry, WorkspacePolicy }
