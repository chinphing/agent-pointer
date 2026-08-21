// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest'
import {
  closestScrollRoot,
  isChartHostLaidOut,
  isInViewForLazyMount,
  isNearViewport,
  mutationTouchesChartHost,
} from './markdownChartMount'

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

function mockBox(
  el: Element,
  box: { width: number; height: number; top: number; bottom: number }
) {
  vi.spyOn(el, 'getBoundingClientRect').mockReturnValue({
    width: box.width,
    height: box.height,
    top: box.top,
    bottom: box.bottom,
    left: 0,
    right: box.width,
    x: 0,
    y: box.top,
    toJSON() {
      return {}
    },
  })
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

  it('does not treat 0×0 hosts as in the viewport', () => {
    const el = document.createElement('div')
    document.body.appendChild(el)
    mockBox(el, { width: 0, height: 0, top: 0, bottom: 0 })
    expect(isChartHostLaidOut(el)).toBe(false)
    expect(isNearViewport(el)).toBe(false)
  })

  it('does not treat laid-out off-screen hosts as in the viewport', () => {
    const el = document.createElement('div')
    document.body.appendChild(el)
    mockBox(el, { width: 400, height: 360, top: 4000, bottom: 4360 })
    expect(isChartHostLaidOut(el)).toBe(true)
    expect(isNearViewport(el)).toBe(false)
  })

  it('treats laid-out on-screen hosts as in the viewport', () => {
    const el = document.createElement('div')
    document.body.appendChild(el)
    mockBox(el, { width: 400, height: 360, top: 40, bottom: 400 })
    expect(isChartHostLaidOut(el)).toBe(true)
    expect(isNearViewport(el)).toBe(true)
  })

  it('uses the file-preview scroller not the window (workspace tab)', () => {
    const scroller = document.createElement('div')
    scroller.className = 'file-preview-scroll'
    const el = document.createElement('div')
    scroller.appendChild(el)
    document.body.appendChild(scroller)
    mockBox(scroller, { width: 400, height: 200, top: 80, bottom: 280 })
    mockBox(el, { width: 400, height: 360, top: 900, bottom: 1260 })
    expect(closestScrollRoot(el)).toBe(scroller)
    expect(isChartHostLaidOut(el)).toBe(true)
    expect(isNearViewport(el)).toBe(false)
    expect(isInViewForLazyMount(el)).toBe(false)
  })
})
