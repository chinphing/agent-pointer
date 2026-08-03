const STORAGE_KEY = 'pointer.chat.lastConversationId'

/** Read the last selected conversation id (app + web localStorage). */
export function readLastConversationId(): string | null {
  try {
    const id = localStorage.getItem(STORAGE_KEY)?.trim()
    return id || null
  } catch {
    return null
  }
}

/** Persist the active conversation id for the next app open. */
export function writeLastConversationId(conversationId: string): void {
  const id = conversationId.trim()
  if (!id) return
  try {
    localStorage.setItem(STORAGE_KEY, id)
  } catch {
    /* ignore quota / private mode */
  }
}

/** Clear when the stored conversation is deleted or no longer restorable. */
export function clearLastConversationId(conversationId?: string): void {
  try {
    if (conversationId) {
      const cur = localStorage.getItem(STORAGE_KEY)?.trim()
      if (cur && cur !== conversationId.trim()) return
    }
    localStorage.removeItem(STORAGE_KEY)
  } catch {
    /* ignore */
  }
}
