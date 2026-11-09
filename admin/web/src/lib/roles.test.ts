import { describe, expect, it } from 'vitest'
import { addableRoles } from './roles'

const ALL = [
  { key: 'super_admin', name: '超级管理员' },
  { key: 'admin', name: '管理员' },
  { key: 'viewer', name: '运营（只读）' },
  { key: 'member', name: '成员' },
]

describe('addableRoles', () => {
  it('列出还没分配的角色', () => {
    expect(addableRoles(ALL, ['member']).map((r) => r.key)).toEqual([
      'super_admin',
      'admin',
      'viewer',
    ])
  })

  it('角色被清空时四个都能加（真机踩过的坑：清空后加不回来）', () => {
    expect(addableRoles(ALL, [])).toHaveLength(4)
  })

  it('全部分配后没有可选项', () => {
    expect(addableRoles(ALL, ['super_admin', 'admin', 'viewer', 'member'])).toEqual([])
  })

  it('忽略不存在的已分配 key，不报错', () => {
    expect(addableRoles(ALL, ['ghost']).map((r) => r.key)).toContain('member')
  })

  it('不改动入参', () => {
    const all = [...ALL]
    addableRoles(all, ['member'])
    expect(all).toHaveLength(4)
  })
})
