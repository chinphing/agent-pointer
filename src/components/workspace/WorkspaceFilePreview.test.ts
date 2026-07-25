// @vitest-environment happy-dom

import { createApp, nextTick } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import WorkspaceFilePreview from './WorkspaceFilePreview.vue'

const mountedApps: Array<ReturnType<typeof createApp>> = []

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

describe('WorkspaceFilePreview', () => {
  it('emits Markdown file references when rendered links are clicked', async () => {
    const openReference = vi.fn()
    const host = document.createElement('div')
    document.body.append(host)
    const app = createApp(WorkspaceFilePreview, {
      preview: {
        path: 'README.md',
        content: '[Guide](docs/guide.md)',
        sizeBytes: 24,
        truncated: false,
        binary: false
      },
      absolutePath: '/workspace/README.md',
      onOpenReference: openReference
    })
    mountedApps.push(app)
    app.mount(host)
    await nextTick()

    const link = host.querySelector('a')
    expect(link?.getAttribute('href')).toBe('docs/guide.md')
    link?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))

    expect(openReference).toHaveBeenCalledWith('docs/guide.md')
  })
})
