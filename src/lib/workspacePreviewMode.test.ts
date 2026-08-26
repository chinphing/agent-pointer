import { describe, expect, it } from 'vitest'
import {
  workspaceFileExtension,
  workspaceRichPreviewAlwaysReady,
  workspaceRichPreviewKind,
  workspaceTextPreviewSurface
} from './workspacePreviewMode'

describe('workspace preview mode', () => {
  it('picks the first path that has a real extension', () => {
    expect(workspaceFileExtension('src/readme.md')).toBe('md')
    expect(workspaceFileExtension(undefined, 'config.JSON', '/abs/config.json')).toBe('json')
    expect(workspaceFileExtension('C:\\work\\a.jsonc')).toBe('jsonc')
    expect(workspaceFileExtension('.gitignore', 'noext')).toBe('')
  })

  it('maps extensions onto registered rich preview kinds', () => {
    expect(workspaceRichPreviewKind('notes.md')).toBe('markdown')
    expect(workspaceRichPreviewKind('a.json')).toBe('json')
    expect(workspaceRichPreviewKind('b.jsonc')).toBe('json')
    expect(workspaceRichPreviewKind('page.html')).toBe('html')
    expect(workspaceRichPreviewKind('index.HTM')).toBe('html')
    expect(workspaceRichPreviewKind('main.ts')).toBeNull()
  })

  it('keeps markdown and HTML always-ready and JSON parse-gated', () => {
    expect(workspaceRichPreviewAlwaysReady('markdown')).toBe(true)
    expect(workspaceRichPreviewAlwaysReady('html')).toBe(true)
    expect(workspaceRichPreviewAlwaysReady('json')).toBe(false)
  })

  it('shows the rich surface only in preview mode when content is ready', () => {
    expect(workspaceTextPreviewSurface('json', 'preview', true)).toBe('json')
    expect(workspaceTextPreviewSurface('json', 'preview', false)).toBe('source')
    expect(workspaceTextPreviewSurface('json', 'source', true)).toBe('source')
    expect(workspaceTextPreviewSurface('markdown', 'preview', true)).toBe('markdown')
    expect(workspaceTextPreviewSurface('html', 'preview', true)).toBe('html')
    expect(workspaceTextPreviewSurface(null, 'preview', true)).toBe('source')
  })
})
