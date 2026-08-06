import { describe, expect, it, beforeEach } from 'vitest'
import {
  composerDropPathsKey,
  normalizeComposerDropPath,
  resetComposerTauriDragDropForTests,
  shouldAcceptComposerDrop,
  uniqueComposerDropPaths
} from './composerTauriDragDrop'

describe('composerTauriDragDrop', () => {
  beforeEach(() => {
    resetComposerTauriDragDropForTests()
  })

  it('normalizes path separators and trailing slashes', () => {
    expect(normalizeComposerDropPath('C:\\Users\\a\\b.txt')).toBe('C:/Users/a/b.txt')
    expect(normalizeComposerDropPath('/tmp/x/')).toBe('/tmp/x')
  })

  it('dedupes duplicate paths in one payload', () => {
    expect(
      uniqueComposerDropPaths([
        '/tmp/a.png',
        '/tmp/a.png',
        '/tmp/a.png/',
        '/tmp/b.png'
      ])
    ).toEqual(['/tmp/a.png', '/tmp/b.png'])
  })

  it('builds a stable key regardless of path order', () => {
    expect(composerDropPathsKey(['/b', '/a'])).toBe(composerDropPathsKey(['/a', '/b']))
  })

  it('rejects the same drop paths within the dedupe window', () => {
    const paths = ['/tmp/one.pdf']
    const first = shouldAcceptComposerDrop(paths, null, 1000)
    expect(first.accept).toBe(true)
    const second = shouldAcceptComposerDrop(paths, first.next, 1200)
    expect(second.accept).toBe(false)
    const later = shouldAcceptComposerDrop(paths, first.next, 2000)
    expect(later.accept).toBe(true)
  })
})
