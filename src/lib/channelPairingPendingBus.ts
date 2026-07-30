import type { PairingPendingItem } from '../lib/channels'

type PairingPendingListener = (item: PairingPendingItem) => void

const listeners = new Set<PairingPendingListener>()

/** Subscribe to live `channel_pairing_pending` stream events (App shell pairing modal). */
export function onChannelPairingPending(listener: PairingPendingListener): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function notifyChannelPairingPending(item: PairingPendingItem): void {
  for (const listener of listeners) {
    try {
      listener(item)
    } catch (e) {
      console.warn('[pairing] listener failed', e)
    }
  }
}
