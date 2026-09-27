// @vitest-environment happy-dom

import { createApp, nextTick } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n, t } from '../../i18n'
import { applyUiLocale } from '../../lib/uiLocale'
import WorkspaceFilePreview from './WorkspaceFilePreview.vue'

const mountedApps: Array<ReturnType<typeof createApp>> = []

beforeEach(() => {
  applyUiLocale('zh-CN')
})

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
  app.use(i18n)
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

  it('uses one source/preview switch for Markdown', async () => {
    const host = mountPreview({
      preview: {
        path: 'README.md',
        content: '# Title\n\nHello',
        sizeBytes: 16,
        truncated: false,
        binary: false
      },
      absolutePath: '/workspace/README.md'
    })
    await nextTick()

    expect(host.querySelectorAll('.file-preview-mode-switch')).toHaveLength(1)
    expect(host.querySelector('.file-preview-markdown')).toBeTruthy()

    const sourceButton = [...host.querySelectorAll('.file-preview-mode-switch button')]
      .find(button => button.textContent === '原文') as HTMLButtonElement
    sourceButton.click()
    await nextTick()
    expect(host.querySelector('.file-preview-markdown')).toBeNull()
    expect(host.querySelector('.file-preview-line-number')).toBeTruthy()
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

  it('shows a collapsible JSON tree and can switch back to source', async () => {
    const host = mountPreview({
      preview: {
        path: 'config.json',
        content: '{"l1":{"l2":{"l3":1}}}',
        sizeBytes: 24,
        truncated: false,
        binary: false
      },
      absolutePath: '/workspace/config.json'
    })
    await nextTick()

    expect(host.textContent).toContain(t('workspace.source'))
    expect(host.textContent).toContain(t('workspace.preview'))
    expect(host.querySelectorAll('.file-preview-mode-switch')).toHaveLength(1)
    expect(host.querySelector('[data-json-node-id="$"]')).toBeTruthy()
    expect(host.textContent).toContain('{1}')
    expect(host.textContent).not.toContain('l3')

    const toggle = host.querySelector('[data-json-node-id="$.l1.l2"] .json-toggle') as HTMLButtonElement
    expect(toggle).toBeTruthy()
    toggle.click()
    await nextTick()
    expect(host.textContent).toContain('l3')

    const sourceButton = [...host.querySelectorAll('.file-preview-mode-switch button')]
      .find(button => button.textContent === t('workspace.source')) as HTMLButtonElement
    sourceButton.click()
    await nextTick()
    expect(host.querySelector('[data-json-node-id="$"]')).toBeNull()
    expect(host.querySelector('.file-preview-line-number')).toBeTruthy()
  })

  it('keeps invalid JSON on the source view without a preview switch', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const host = mountPreview({
      preview: {
        path: 'broken.json',
        content: '{',
        sizeBytes: 1,
        truncated: false,
        binary: false
      },
      absolutePath: '/workspace/broken.json'
    })
    await nextTick()

    expect(host.textContent).not.toContain(t('workspace.preview'))
    expect(host.querySelector('.file-preview-line-number')).toBeTruthy()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  it('uses one source/preview switch for HTML', async () => {
    const host = mountPreview({
      preview: {
        path: 'page.html',
        content: '<h1>Hello</h1>',
        sizeBytes: 15,
        truncated: false,
        binary: false
      },
      absolutePath: '/workspace/page.html'
    })
    await nextTick()

    expect(host.querySelectorAll('.file-preview-mode-switch')).toHaveLength(1)
    const iframe = host.querySelector('.file-preview-html iframe') as HTMLIFrameElement | null
    expect(iframe).toBeTruthy()
    expect(iframe?.getAttribute('sandbox')).toBe('allow-scripts allow-modals')
    expect(iframe?.srcdoc || iframe?.getAttribute('srcdoc') || '').toContain('<h1>Hello</h1>')
    expect(iframe?.srcdoc || iframe?.getAttribute('srcdoc') || '').toContain('charset="utf-8"')

    const sourceButton = [...host.querySelectorAll('.file-preview-mode-switch button')]
      .find(button => button.textContent === t('workspace.source')) as HTMLButtonElement
    sourceButton.click()
    await nextTick()
    expect(host.querySelector('.file-preview-html iframe')).toBeNull()
    expect(host.querySelector('.file-preview-line-number')).toBeTruthy()
  })
})
