import { describe, expect, it } from 'vitest'
import { parseAskUserArgs, parseAskUserSelection } from './askUser'

describe('askUser parsing', () => {
  it('parses options and multi-select mode', () => {
    expect(parseAskUserArgs(JSON.stringify({
      question: 'Choose targets',
      options: [{ label: 'App' }, { label: 'Web', description: 'Browser' }],
      multi_select: true
    }))).toEqual({
      question: 'Choose targets',
      options: [{ label: 'App', description: undefined }, { label: 'Web', description: 'Browser' }],
      multiSelect: true
    })
  })

  it('rejects malformed questions', () => {
    expect(parseAskUserArgs('{bad')).toBeNull()
    expect(parseAskUserArgs(JSON.stringify({ question: 'Choose', options: [{ label: 'Only' }] }))).toBeNull()
  })

  it('reads the completed selection', () => {
    expect(parseAskUserSelection('{"selected":["App","Web"]}')).toEqual(['App', 'Web'])
  })
})
