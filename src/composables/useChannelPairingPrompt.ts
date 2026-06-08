import { onMounted, onUnmounted, ref } from 'vue'
import {
  listAllChannelPairingPending,
  type PairingPendingItem
} from '../lib/channels'

const PAIRING_POLL_MS = 4_000

function pairingKey(item: PairingPendingItem): string {
  return `${item.channel}:${item.code}`
}

export function useChannelPairingPrompt() {
  const open = ref(false)
  const pendingItem = ref<PairingPendingItem | null>(null)
  const dismissedKeys = new Set<string>()

  let pollId: number | undefined

  async function poll() {
    try {
      const pending = await listAllChannelPairingPending('default')
      if (open.value && pendingItem.value) {
        const currentKey = pairingKey(pendingItem.value)
        const stillPending = pending.some(p => pairingKey(p) === currentKey)
        if (!stillPending) {
          open.value = false
          pendingItem.value = null
        }
      }
      if (open.value) return

      const next = pending.find(p => !dismissedKeys.has(pairingKey(p)))
      if (next) {
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
