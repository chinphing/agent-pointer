import { onMounted, onUnmounted, ref } from 'vue'
import {
  listAllChannelPairingPending,
  type PairingPendingItem
} from '../lib/channels'
import { onChannelPairingPending } from '../lib/channelPairingPendingBus'

const PAIRING_POLL_MS = 4_000
/** Match backend pending TTL (5 min). */
const PAIRING_TTL_SECS = 5 * 60
/** Only prompt for recently issued codes (must be ≤ TTL). */
const PAIRING_FRESH_SECS = 5 * 60

function pairingKey(item: PairingPendingItem): string {
  return `${item.channel}:${item.code}`
}

function isFreshPending(item: PairingPendingItem): boolean {
  const issuedAt = item.issuedAt ?? 0
  if (issuedAt <= 0) return false
  const age = Math.floor(Date.now() / 1000) - issuedAt
  return age >= 0 && age <= PAIRING_FRESH_SECS
}

function deadlineMsForItem(item: PairingPendingItem): number {
  const issuedAt = item.issuedAt ?? 0
  const baseMs = issuedAt > 0 ? issuedAt * 1000 : Date.now()
  return baseMs + PAIRING_TTL_SECS * 1000
}

export function useChannelPairingPrompt() {
  const open = ref(false)
  const pendingItem = ref<PairingPendingItem | null>(null)
  const dismissedKeys = new Set<string>()
  /** 本会话已提示过的配对，避免重复弹窗 */
  const promptedKeys = new Set<string>()

  let pollId: number | undefined
  let pollDeadlineMs = 0
  let unsubIssued: (() => void) | undefined

  function stopPolling(reason: string) {
    if (pollId === undefined) return
    window.clearInterval(pollId)
    pollId = undefined
    pollDeadlineMs = 0
    console.info('[pairing] poll stopped', reason)
  }

  function startPolling(deadlineMs: number) {
    pollDeadlineMs = Math.max(pollDeadlineMs, deadlineMs)
    if (pollId !== undefined) return
    pollId = window.setInterval(() => void pollTick(), PAIRING_POLL_MS)
    console.info('[pairing] poll started', {
      until: new Date(pollDeadlineMs).toISOString()
    })
  }

  function armForItem(item: PairingPendingItem) {
    startPolling(deadlineMsForItem(item))
  }

  function maybeOpenModal(item: PairingPendingItem) {
    if (open.value) return
    if (!isFreshPending(item)) return
    const key = pairingKey(item)
    if (dismissedKeys.has(key) || promptedKeys.has(key)) return
    promptedKeys.add(key)
    pendingItem.value = item
    open.value = true
  }

  async function pollTick() {
    if (pollDeadlineMs > 0 && Date.now() > pollDeadlineMs) {
      stopPolling('ttl')
      if (open.value) {
        open.value = false
        pendingItem.value = null
      }
      return
    }

    try {
      const pending = await listAllChannelPairingPending('default')
      const fresh = pending.filter(p => isFreshPending(p))

      if (open.value && pendingItem.value) {
        const currentKey = pairingKey(pendingItem.value)
        const stillPending = fresh.some(p => pairingKey(p) === currentKey)
        if (!stillPending) {
          open.value = false
          pendingItem.value = null
        }
      }

      if (!open.value) {
        const next = fresh.find(
          p => !dismissedKeys.has(pairingKey(p)) && !promptedKeys.has(pairingKey(p))
        )
        if (next) {
          maybeOpenModal(next)
        }
      }

      if (fresh.length === 0 && !open.value) {
        stopPolling('empty')
      }
    } catch (e) {
      console.warn('[pairing] poll pending failed', e)
    }
  }

  function dismiss() {
    if (pendingItem.value) {
      dismissedKeys.add(pairingKey(pendingItem.value))
    }
    open.value = false
    pendingItem.value = null
    void pollTick()
  }

  function onApproved() {
    if (pendingItem.value) {
      dismissedKeys.add(pairingKey(pendingItem.value))
    }
    open.value = false
    pendingItem.value = null
    void pollTick()
  }

  onMounted(() => {
    unsubIssued = onChannelPairingPending(item => {
      armForItem(item)
      maybeOpenModal(item)
    })
    // Boot: only arm polling when a fresh pending code already exists.
    void (async () => {
      try {
        const pending = await listAllChannelPairingPending('default')
        const fresh = pending.filter(p => isFreshPending(p))
        if (fresh.length === 0) return
        for (const item of fresh) armForItem(item)
        const next = fresh.find(
          p => !dismissedKeys.has(pairingKey(p)) && !promptedKeys.has(pairingKey(p))
        )
        if (next) maybeOpenModal(next)
      } catch (e) {
        console.warn('[pairing] boot pending check failed', e)
      }
    })()
  })

  onUnmounted(() => {
    unsubIssued?.()
    unsubIssued = undefined
    stopPolling('unmount')
  })

  return { open, pendingItem, dismiss, onApproved }
}
