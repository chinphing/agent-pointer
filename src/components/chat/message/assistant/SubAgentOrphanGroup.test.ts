// @vitest-environment happy-dom

import { createApp, h, type App } from 'vue'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { i18n } from '../../../../i18n'
import { applyUiLocale } from '../../../../lib/uiLocale'
import SubAgentOrphanGroup from './SubAgentOrphanGroup.vue'

const mountedApps: App[] = []

function mountGroup(count: number, slotText = '', showLabel?: boolean): HTMLElement {
  const host = document.createElement('div')
  document.body.append(host)
  const app = createApp({
    render: () =>
      h(
        SubAgentOrphanGroup,
        showLabel === undefined ? { count } : { count, showLabel },
        { default: () => h('span', slotText) }
      )
  })
  app.use(i18n)
  mountedApps.push(app)
  app.mount(host)
  return host
}

function labelText(host: HTMLElement): string {
  return host.querySelector('[data-testid="sub-agent-orphan-label"]')?.textContent ?? ''
}

beforeEach(() => {
  applyUiLocale('zh-CN')
})

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

describe('SubAgentOrphanGroup', () => {
  it('未关联组显式标注并带条数，不再无声追加', () => {
    const host = mountGroup(3, 'frame-body')
    const text = labelText(host)
    expect(text).toContain('未关联的子 agent')
    expect(text).toContain('3')
    // 帧本体仍然渲染在标注下面，不是被吞掉。
    expect(host.textContent).toContain('frame-body')
  })

  it('条数随入参变化（一条也要标注）', () => {
    const host = mountGroup(1)
    expect(labelText(host)).toContain('1')
  })

  it('showLabel: false 时只保留容器与帧，不报「未关联」', () => {
    // 宿主行**存在**、只是被折叠投影隐藏（unplaced）：帧原地照常显示，但不该报警。
    const host = mountGroup(0, 'frame-body', false)
    expect(labelText(host)).toBe('')
    expect(host.textContent).toContain('frame-body')
  })
})
