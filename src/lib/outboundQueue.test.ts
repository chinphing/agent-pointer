import { describe, expect, it } from 'vitest'
import { promoteOutboundQueueItem } from './outboundQueue'
import type { OutboundQueueItem } from '../types/chat'

function item(id: string, content = id): OutboundQueueItem {
  return { id, content, createdAt: 1 }
}

describe('promoteOutboundQueueItem', () => {
  it('moves a middle item to the front', () => {
    const q = [item('a'), item('b'), item('c')]
    expect(promoteOutboundQueueItem(q, 'b').map(i => i.id)).toEqual(['b', 'a', 'c'])
  })

  it('is a no-op when already first or missing', () => {
    const q = [item('a'), item('b')]
    expect(promoteOutboundQueueItem(q, 'a')).toBe(q)
    expect(promoteOutboundQueueItem(q, 'missing')).toBe(q)
  })
})
