// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import { mutationTouchesChartHost } from './markdownChartMount'

function childListMutation(added: Node[], removed: Node[] = []): MutationRecord {
  return {
    type: 'childList',
    target: document.createElement('div'),
    addedNodes: added as unknown as NodeList,
    removedNodes: removed as unknown as NodeList,
    attributeName: null,
    attributeNamespace: null,
    nextSibling: null,
    oldValue: null,
    previousSibling: null,
  }
}

describe('markdownChartMount', () => {
  it('detects a newly inserted chart host', () => {
    const host = document.createElement('div')
    host.className = 'md-chart'
    expect(mutationTouchesChartHost([childListMutation([host])])).toBe(true)
  })

  it('detects a chart host nested in replaced markdown', () => {
    const wrap = document.createElement('div')
    const host = document.createElement('div')
    host.className = 'md-chart'
    wrap.appendChild(host)
    expect(mutationTouchesChartHost([childListMutation([wrap])])).toBe(true)
  })

  it('ignores canvas rebuilds inside an existing host', () => {
    const canvas = document.createElement('canvas')
    expect(mutationTouchesChartHost([childListMutation([canvas])])).toBe(false)
  })
})
