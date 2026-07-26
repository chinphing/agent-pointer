import { describe, expect, it } from 'vitest'
import {
  endOfFirstJsonValue,
  parseToolCallArguments,
  parseToolCallArgumentsObject
} from './parseToolCallArguments'
import { fileEditSnippetFromArgs } from './textDiff'

describe('parseToolCallArguments', () => {
  it('parses plain objects', () => {
    expect(parseToolCallArgumentsObject('{"path":"a.py","oldString":"x","newString":"y"}')).toEqual({
      path: 'a.py',
      oldString: 'x',
      newString: 'y'
    })
  })

  it('tolerates trailing junk after a complete object', () => {
    const raw =
      '{"newString":"a","oldString":"b","path":"scripts/prepare_reimbursement_draft.py"}}'
    expect(parseToolCallArgumentsObject(raw)).toEqual({
      newString: 'a',
      oldString: 'b',
      path: 'scripts/prepare_reimbursement_draft.py'
    })
    expect(fileEditSnippetFromArgs(raw)).toEqual({ oldString: 'b', newString: 'a' })
  })

  it('unwraps JSON string payloads', () => {
    const inner = '{"path":"x","oldString":"a","newString":"b"}'
    expect(parseToolCallArgumentsObject(JSON.stringify(inner))).toEqual({
      path: 'x',
      oldString: 'a',
      newString: 'b'
    })
  })

  it('strips markdown fences', () => {
    expect(parseToolCallArgumentsObject('```json\n{"path":"z"}\n```')).toEqual({ path: 'z' })
  })

  it('endOfFirstJsonValue stops before trailing brace', () => {
    const s = '{"a":1}}'
    expect(endOfFirstJsonValue(s)).toBe(7)
    expect(parseToolCallArguments(s)).toEqual({ a: 1 })
  })
})
