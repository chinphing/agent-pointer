import { defineStore } from 'pinia'
import { ref } from 'vue'
import { setMessageMilestone } from '../lib/api'
import type { ConversationOutlineItem } from '../types/chat'

function pendingKey(conversationId: string, messageId: string): string {
  return `${conversationId}\0${messageId}`
}

export const useMessageMilestoneStore = defineStore('messageMilestones', () => {
  const flags = ref<Record<string, Record<string, true>>>({})
  const pending = ref<Record<string, true>>({})

  function isMilestone(conversationId: string, messageId: string): boolean {
    return flags.value[conversationId]?.[messageId] === true
  }

  function isPending(conversationId: string, messageId: string): boolean {
    return pending.value[pendingKey(conversationId, messageId)] === true
  }

  /** Replace one conversation's flags from the outline. In-flight toggles win. */
  function hydrate(conversationId: string, items: readonly ConversationOutlineItem[]) {
    const id = conversationId.trim()
    if (!id) {
      console.warn('[milestone] hydrate skipped; empty conversation id')
      return
    }
    const prev = flags.value[id] ?? {}
    const next: Record<string, true> = {}
    const listed = new Set(items.map(item => item.messageId))
    for (const item of items) {
      const dirty = pending.value[pendingKey(id, item.messageId)] === true
      if (dirty) {
        if (prev[item.messageId]) next[item.messageId] = true
        continue
      }
      if (item.milestone) next[item.messageId] = true
    }
    for (const messageId of Object.keys(prev)) {
      if (listed.has(messageId)) continue
      if (prev[messageId] && pending.value[pendingKey(id, messageId)]) {
        next[messageId] = true
      }
    }
    flags.value = { ...flags.value, [id]: next }
  }

  async function setMilestone(
    conversationId: string,
    messageId: string,
    milestone: boolean
  ): Promise<void> {
    const id = conversationId.trim()
    const message = messageId.trim()
    if (!id || !message) {
      console.warn('[milestone] set skipped; missing id', conversationId, messageId)
      return
    }
    const key = pendingKey(id, message)
    if (pending.value[key]) {
      console.warn('[milestone] set skipped; request in flight', id, message)
      return
    }
    const prevOn = flags.value[id]?.[message] === true
    const conv = { ...(flags.value[id] ?? {}) }
    if (milestone) conv[message] = true
    else delete conv[message]
    flags.value = { ...flags.value, [id]: conv }
    pending.value = { ...pending.value, [key]: true }
    try {
      await setMessageMilestone(id, message, milestone)
      console.info('[milestone] set', id, message, milestone)
    } catch (err) {
      const revert = { ...(flags.value[id] ?? {}) }
      if (prevOn) revert[message] = true
      else delete revert[message]
      flags.value = { ...flags.value, [id]: revert }
      console.warn('[milestone] set failed', id, message, milestone, err)
    } finally {
      const nextPending = { ...pending.value }
      delete nextPending[key]
      pending.value = nextPending
    }
  }

  return { isMilestone, isPending, hydrate, setMilestone }
})
