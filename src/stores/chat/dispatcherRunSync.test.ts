import { describe, expect, it } from 'vitest'
import { activeConversationIdsFromQueueSnapshot } from './dispatcherRunSync'
import type { RunQueueSnapshot } from '../../types/automation'

describe('activeConversationIdsFromQueueSnapshot', () => {
  it('collects active session lanes, waiters, and pending runs', () => {
    const snapshot: RunQueueSnapshot = {
      maxConcurrentMain: 4,
      maxConcurrentCron: 4,
      lanes: [
        {
          lane: 'session:conv-active',
          active: 1,
          waiting: 0,
          maxConcurrent: 1,
          waiters: [],
        },
        {
          lane: 'session:conv-queued',
          active: 0,
          waiting: 1,
          maxConcurrent: 1,
          waiters: [
            {
              runId: 'run-1',
              conversationId: 'conv-queued',
              triggerSource: 'http_runs',
            },
          ],
        },
      ],
      pendingRuns: [
        {
          runId: 'run-2',
          conversationId: 'conv-pending',
          triggerSource: 'ipc',
          createdAtMs: 1,
        },
      ],
    }

    const ids = activeConversationIdsFromQueueSnapshot(snapshot)
    expect(ids.has('conv-active')).toBe(true)
    expect(ids.has('conv-queued')).toBe(true)
    expect(ids.has('conv-pending')).toBe(true)
  })
})
