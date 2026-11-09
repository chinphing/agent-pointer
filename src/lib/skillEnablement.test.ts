import { describe, expect, it } from 'vitest'
import { mergeOrgDefaultOn } from './skillEnablement'

describe('mergeOrgDefaultOn', () => {
  it('把企业默认技能补进列表', () => {
    expect(mergeOrgDefaultOn(['find-skills'], ['travel'], [])).toEqual(['find-skills', 'travel'])
  })

  it('已在列表里的不重复添加', () => {
    expect(mergeOrgDefaultOn(['travel'], ['travel'], [])).toEqual(['travel'])
  })

  it('员工显式关过的不加回来', () => {
    expect(mergeOrgDefaultOn(['find-skills'], ['travel'], ['travel'])).toEqual(['find-skills'])
  })

  it('老 override 里没有企业技能时仍会补上（这是真机踩到的坑）', () => {
    // 员工的内置技能 override 建于企业技能下发之前，里面没有 travel，
    // 但这不代表他关掉了它——企业默认必须生效。
    const staleOverride = ['find-skills', 'dev-env-setup', 'skill-manager']
    expect(mergeOrgDefaultOn(staleOverride, ['travel'], [])).toContain('travel')
  })

  it('多个企业技能按 opt-out 分别处理', () => {
    expect(mergeOrgDefaultOn([], ['travel', 'expense', 'meeting'], ['expense'])).toEqual([
      'travel',
      'meeting'
    ])
  })

  it('不改动入参', () => {
    const input = ['a']
    mergeOrgDefaultOn(input, ['b'], [])
    expect(input).toEqual(['a'])
  })
})
