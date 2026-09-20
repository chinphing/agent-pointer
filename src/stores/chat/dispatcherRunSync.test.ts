import { describe, expect, it } from 'vitest'
import { activeConversationIdsFromQueueSnapshot, backgroundJobOccupancyFromQueueSnapshot } from './dispatcherRunSync'
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

  it('backgroundJobOccupancyFromQueueSnapshot treats missing field as unknown', () => {
    const snapshot: RunQueueSnapshot = {
      maxConcurrentMain: 4,
      maxConcurrentCron: 4,
      lanes: [],
      pendingRuns: []
    }
    expect(backgroundJobOccupancyFromQueueSnapshot(snapshot)).toBeNull()
  })

  it('backgroundJobOccupancyFromQueueSnapshot maps empty list to no occupancy', () => {
    const snapshot: RunQueueSnapshot = {
      maxConcurrentMain: 4,
      maxConcurrentCron: 4,
      lanes: [],
      pendingRuns: [],
      backgroundJobs: []
    }
    const occupancy = backgroundJobOccupancyFromQueueSnapshot(snapshot)
    expect(occupancy).not.toBeNull()
    expect(occupancy?.size).toBe(0)
  })

  it('backgroundJobOccupancyFromQueueSnapshot keeps nested job titles', () => {
    const snapshot: RunQueueSnapshot = {
      maxConcurrentMain: 4,
      maxConcurrentCron: 4,
      lanes: [],
      pendingRuns: [],
      backgroundJobs: [
        {
          conversationId: 'conv-a',
          runningCount: 1,
          jobs: [
            {
              jobId: 'job_term',
              status: 'running',
              kind: 'terminal',
              title: 'python scrape.py'
            }
          ]
        }
      ]
    }
    const occupancy = backgroundJobOccupancyFromQueueSnapshot(snapshot)
    expect(occupancy?.get('conv-a')).toEqual({
      count: 1,
      jobs: [
        {
          jobId: 'job_term',
          status: 'running',
          kind: 'terminal',
          title: 'python scrape.py'
        }
      ]
    })
  })
})
