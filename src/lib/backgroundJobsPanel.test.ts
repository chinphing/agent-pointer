import { beforeAll, describe, expect, it } from 'vitest'
import type { BackgroundJobView } from '../types/chat'
import { i18n } from '../i18n'
import { collectBackgroundJobsPanelItems } from './backgroundJobsPanel'

beforeAll(() => {
  // Fallback labels assert against zh-CN catalog until tests cover both locales.
  i18n.global.locale.value = 'zh-CN'
})

describe('collectBackgroundJobsPanelItems', () => {
  it('lists occupancy jobs including nested background terminals', () => {
    const items = collectBackgroundJobsPanelItems([
      {
        jobId: 'job_sub',
        status: 'running',
        kind: 'subagent',
        title: '铺线 1',
        agentId: 'self'
      },
      {
        jobId: 'job_term',
        status: 'running',
        kind: 'terminal',
        title: 'python scrape.py'
      }
    ])
    expect(items).toHaveLength(2)
    expect(items[0]).toMatchObject({ kind: 'host', jobId: 'job_sub', title: '铺线 1' })
    expect(items[1]).toMatchObject({
      kind: 'host',
      jobId: 'job_term',
      title: 'python scrape.py',
      jobKind: 'terminal'
    })
  })

  it('falls back to a localized title when the job has none', () => {
    const items = collectBackgroundJobsPanelItems([
      { jobId: 'job_a', status: 'running', kind: 'subagent', title: '   ' },
      { jobId: 'job_b', status: 'running', kind: 'terminal' }
    ])
    expect(items.map(i => i.title)).toEqual(['子任务', '终端'])
  })

  it('returns no rows for empty or missing occupancy', () => {
    expect(collectBackgroundJobsPanelItems(undefined)).toEqual([])
    expect(collectBackgroundJobsPanelItems([])).toEqual([])
  })

  it('row count always equals occupancy length (no await rows)', () => {
    // `job.await` 不再进面板：它的「结束等待」按钮在消息里的工具行上。
    // 面板行数必须与 `backgroundJobCount`（= occupancy.length）完全一致。
    const occupancy: BackgroundJobView[] = [
      { jobId: 'job_1', status: 'running', kind: 'subagent', title: 'a' },
      { jobId: 'job_2', status: 'running', kind: 'terminal', title: 'b' },
      { jobId: 'job_3', status: 'running', kind: 'terminal' }
    ]
    const items = collectBackgroundJobsPanelItems(occupancy)
    expect(items).toHaveLength(occupancy.length)
    expect(items.map(i => i.kind)).toEqual(['host', 'host', 'host'])
    expect(items.map(i => i.key)).toEqual(['job_1', 'job_2', 'job_3'])
  })
})
