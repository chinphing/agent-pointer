// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import {
  __resetSidebarSectionCollapseForTests,
  useSidebarSectionCollapse
} from './useSidebarSectionCollapse'

const STORAGE_KEY = 'pointer.sidebar.sectionCollapse'

describe('useSidebarSectionCollapse', () => {
  beforeEach(() => {
    localStorage.clear()
    __resetSidebarSectionCollapseForTests()
  })

  it('defaults all sections expanded', () => {
    const { sectionCollapse } = useSidebarSectionCollapse()
    expect(sectionCollapse.value).toEqual({
      pinned: false,
      projects: false,
      conversations: false
    })
  })

  it('persists toggles to localStorage', async () => {
    const api = useSidebarSectionCollapse()
    api.toggleProjects()
    api.toggleConversations()
    await nextTick()
    const raw = localStorage.getItem(STORAGE_KEY)
    expect(raw).toBeTruthy()
    expect(JSON.parse(raw!)).toEqual({
      pinned: false,
      projects: true,
      conversations: true
    })
  })

  it('expandProjects clears collapsed projects section', () => {
    const api = useSidebarSectionCollapse()
    api.toggleProjects()
    expect(api.sectionCollapse.value.projects).toBe(true)
    api.expandProjects()
    expect(api.sectionCollapse.value.projects).toBe(false)
  })
})
