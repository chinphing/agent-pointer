import { onMounted, onUnmounted, ref } from 'vue'
import {
  listAllChannelPairingPending,
  type PairingPendingItem
} from '../lib/channels'

const PAIRING_POLL_MS = 4_000
/** 与后端 pending TTL（30min）对齐，仅对近期配对弹窗 */
const PAIRING_FRESH_SECS = 15 * 60

function pairingKey(item: PairingPendingItem): string {
  return `${item.channel}:${item.code}`
}

function isFreshPending(item: PairingPendingItem): boolean {
  const issuedAt = item.issuedAt ?? 0
  if (issuedAt <= 0) return false
  const age = Math.floor(Date.now() / 1000) - issuedAt
  return age >= 0 && age <= PAIRING_FRESH_SECS
}

export function useChannelPairingPrompt() {
  const open = ref(false)
  const pendingItem = ref<PairingPendingItem | null>(null)
  const dismissedKeys = new Set<string>()
  /** 本会话已提示过的配对，避免重复弹窗 */
  const promptedKeys = new Set<string>()

  let pollId: number | undefined

  async function poll() {
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
      if (open.value) return

      const next = fresh.find(
        p => !dismissedKeys.has(pairingKey(p)) && !promptedKeys.has(pairingKey(p))
      )
      if (next) {
        promptedKeys.add(pairingKey(next))
        pendingItem.value = next
        open.value = true
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
  }

  function onApproved() {
    if (pendingItem.value) {
      dismissedKeys.add(pairingKey(pendingItem.value))
    }
    open.value = false
    pendingItem.value = null
    void poll()
  }

  onMounted(() => {
    void poll()
    pollId = window.setInterval(() => void poll(), PAIRING_POLL_MS)
  })

  onUnmounted(() => {
    if (pollId !== undefined) {
      window.clearInterval(pollId)
    }
  })

  return { open, pendingItem, dismiss, onApproved }
}
