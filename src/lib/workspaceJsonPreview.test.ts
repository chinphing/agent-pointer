import { describe, expect, it } from 'vitest'
import {
  collectJsonAncestorIds,
  defaultCollapsedIds,
  findJsonPreviewMatches,
  isJsonArrayItemId,
  isJsonPreviewPath,
  jsonCollapsedSummary,
  jsonPreviewSearchParts,
  parseJsonPreview
} from './workspaceJsonPreview'

describe('workspace JSON preview', () => {
  it('parses nested objects and arrays', () => {
    const result = parseJsonPreview('{"name":"a","tags":[1,true,null]}')
    expect(result.ok).toBe(true)
    if (!result.ok) return
    expect(result.root.kind).toBe('object')
    expect(result.root.children).toHaveLength(2)
    expect(result.root.children[0]?.key).toBe('name')
    expect(result.root.children[0]?.text).toBe('"a"')
    expect(result.root.children[1]?.kind).toBe('array')
    expect(result.root.children[1]?.children.map(child => child.text)).toEqual([
      '1',
      'true',
      'null'
    ])
  })

  it('maps json paths through the shared preview registry', () => {
    expect(isJsonPreviewPath('a.json')).toBe(true)
    expect(isJsonPreviewPath('a.jsonc')).toBe(true)
    expect(isJsonPreviewPath('a.md')).toBe(false)
  })

  it('rejects invalid JSON', () => {
    expect(parseJsonPreview('{')).toEqual({ ok: false, reason: 'invalid' })
  })

  it('rejects trees that exceed the node cap', () => {
    expect(parseJsonPreview('[1,2,3]', 2)).toEqual({ ok: false, reason: 'too_large' })
  })

  it('does not treat nested object keys as array indexes', () => {
    const result = parseJsonPreview('{"items":[{"name":"x"}]}')
    expect(result.ok).toBe(true)
    if (!result.ok) return
    const name = result.root.children[0]?.children[0]?.children[0]
    expect(name?.key).toBe('name')
    expect(name?.id).toBe('$.items[0].name')
    expect(isJsonArrayItemId(name!.id)).toBe(false)
    expect(isJsonArrayItemId('$.items[0]')).toBe(true)
    expect(findJsonPreviewMatches(result.root, 'name')[0]).toMatchObject({
      nodeId: '$.items[0].name',
      field: 'key'
    })
    expect(findJsonPreviewMatches(result.root, '0')).toEqual([])
  })

  it('summarizes collapsed containers', () => {
    const result = parseJsonPreview('{"a":1,"b":[1,2,3]}')
    expect(result.ok).toBe(true)
    if (!result.ok) return
    expect(jsonCollapsedSummary(result.root)).toBe('{2}')
    expect(jsonCollapsedSummary(result.root.children[1]!)).toBe('[3]')
  })

  it('collapses containers deeper than the expand depth', () => {
    const result = parseJsonPreview('{"l1":{"l2":{"l3":1}}}')
    expect(result.ok).toBe(true)
    if (!result.ok) return
    const collapsed = defaultCollapsedIds(result.root, 2)
    expect(collapsed.has('$')).toBe(false)
    expect(collapsed.has('$.l1')).toBe(false)
    expect(collapsed.has('$.l1.l2')).toBe(true)
  })

  it('finds keys and values and reports ancestor ids for expand-on-match', () => {
    const result = parseJsonPreview('{"user":{"email":"a@b.com"}}')
    expect(result.ok).toBe(true)
    if (!result.ok) return
    const matches = findJsonPreviewMatches(result.root, 'email')
    expect(matches).toEqual([
      { nodeId: '$.user.email', field: 'key', start: 0, end: 5 }
    ])
    expect(collectJsonAncestorIds(result.root, '$.user.email')).toEqual(['$.user', '$'])
    const valueMatches = findJsonPreviewMatches(result.root, 'a@b')
    expect(valueMatches[0]).toMatchObject({ nodeId: '$.user.email', field: 'value' })
    expect(jsonPreviewSearchParts('"a@b.com"', '$.user.email', 'value', valueMatches).some(
      part => part.matchIndex === 0
    )).toBe(true)
  })
})
