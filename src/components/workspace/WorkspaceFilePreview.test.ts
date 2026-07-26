// @vitest-environment happy-dom

import { createApp, nextTick } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import WorkspaceFilePreview from './WorkspaceFilePreview.vue'

const mountedApps: Array<ReturnType<typeof createApp>> = []

afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount()
  document.body.innerHTML = ''
})

function mountPreview(props: Record<string, unknown>) {
  const host = document.createElement('div')
  const panel = document.createElement('div')
  panel.setAttribute('data-workspace-panel', '')
  document.body.append(panel)
  panel.append(host)
  const app = createApp(WorkspaceFilePreview, props)
  mountedApps.push(app)
  app.mount(host)
  return host
}

describe('WorkspaceFilePreview', () => {
  it('emits Markdown file references when rendered links are clicked', async () => {
    const openReference = vi.fn()
    const host = mountPreview({
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
    await nextTick()

    const link = host.querySelector('a')
    expect(link?.getAttribute('href')).toBe('docs/guide.md')
    link?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))

    expect(openReference).toHaveBeenCalledWith('docs/guide.md')
  })

  it('opens find on ⌘/Ctrl+F and highlights source matches', async () => {
    const host = mountPreview({
      preview: {
        path: 'main.ts',
        content: 'const alpha = 1\nconst beta = alpha\n',
        sizeBytes: 32,
        truncated: false,
        binary: false
      },
      absolutePath: '/workspace/main.ts'
    })
    await nextTick()

    const preview = host.querySelector('[data-workspace-file-preview]') as HTMLElement
    preview.focus()
    window.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'f',
      metaKey: true,
      bubbles: true,
      cancelable: true
    }))
    await nextTick()

    const input = host.querySelector('.file-preview-search input') as HTMLInputElement | null
    expect(input).toBeTruthy()
    input!.value = 'alpha'
    input!.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()

    const marks = host.querySelectorAll('[data-file-search-match]')
    expect(marks).toHaveLength(2)
    expect(host.textContent).toContain('1/2')
  })
})
