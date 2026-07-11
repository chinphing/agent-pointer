import { describe, expect, it } from 'vitest'
import type { TaskBoardItem } from '../types/chat'
import { milestoneTitle, milestoneRemark, milestoneRowLabel } from './taskBoardDisplay'

describe('taskBoardDisplay', () => {
  it('shows title only', () => {
    const item: TaskBoardItem = {
      id: 'g_plan',
      title: '读取 Excel 并打开 BOSS 职位发布页',
      status: 'done',
      done_when: 'criteria not shown in UI'
    }
    expect(milestoneTitle(item)).toBe('读取 Excel 并打开 BOSS 职位发布页')
  })

  it('falls back to id when title empty', () => {
    const item: TaskBoardItem = { id: 'm1', title: '', status: 'pending' }
    expect(milestoneTitle(item)).toBe('m1')
  })

  it('shows remark on terminal rows only', () => {
    const done: TaskBoardItem = {
      id: 'wi_4',
      title: '#4',
      status: 'done',
      remark: '13145678901 - 不存在'
    }
    expect(milestoneRemark(done)).toBe('13145678901 - 不存在')
    const pending: TaskBoardItem = {
      id: 'wi_5',
      title: '#5',
      status: 'in_progress',
      remark: 'should hide'
    }
    expect(milestoneRemark(pending)).toBeNull()
  })

  it('joins title and remark on one line', () => {
    const done: TaskBoardItem = {
      id: 'wi_1',
      title: '#1',
      status: 'done',
      remark: '13812345678 - 用户不存在'
    }
    expect(milestoneRowLabel(done)).toBe('#1 13812345678 - 用户不存在')
  })
})
