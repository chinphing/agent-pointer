import { describe, expect, it } from 'vitest'
import { mapListResponse, parseResultSummary } from './useWorkItemsList'

describe('useWorkItemsList helpers', () => {
  it('parseResultSummary reads summary from json string', () => {
    expect(parseResultSummary('{"summary":"opened app"}')).toBe('opened app')
  })

  it('mapListResponse normalizes api payload', () => {
    const parsed = mapListResponse({
      total: 2,
      items: [
        { id: 'wi_1', batch_id: 'batch_a', seq: 1, title: 'One', status: 'done', result_summary: null },
        { id: 'wi_2', batch_id: 'batch_a', seq: 2, title: 'Two', status: 'pending', result_summary: '{"summary":"x"}' }
      ]
    })
    expect(parsed.total).toBe(2)
    expect(parsed.items[1].resultSummary).toBe('x')
  })
})
