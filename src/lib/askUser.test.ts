import { describe, expect, it } from 'vitest'
import { parseAskUserArgs, parseAskUserSelection, parseAskUserSummary } from './askUser'

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

  it('coerces stringified options and ignores trailing junk', () => {
    const options = JSON.stringify([
      { label: '允许' },
      { label: '仅步骤', description: '不操作桌面' }
    ])
    expect(parseAskUserArgs(JSON.stringify({
      question: '是否允许桌面控制？',
      options: `${options}]`
    }))).toEqual({
      question: '是否允许桌面控制？',
      options: [
        { label: '允许', description: undefined },
        { label: '仅步骤', description: '不操作桌面' }
      ],
      multiSelect: false
    })
  })

  it('reads a display summary when arguments are empty', () => {
    expect(parseAskUserSummary('是否允许桌面控制？\n1. 允许\n2. 仅步骤 - 不操作桌面')).toEqual({
      question: '是否允许桌面控制？',
      options: [
        { label: '允许', description: undefined },
        { label: '仅步骤', description: '不操作桌面' }
      ],
      multiSelect: false
    })
    expect(parseAskUserSummary('只有问题')).toBeNull()
  })

  it('rejects malformed questions', () => {
    expect(parseAskUserArgs('{bad')).toBeNull()
    expect(parseAskUserArgs(JSON.stringify({ question: 'Choose', options: [{ label: 'Only' }] }))).toBeNull()
  })

  it('reads the completed selection', () => {
    expect(parseAskUserSelection('{"selected":["App","Web"]}')).toEqual(['App', 'Web'])
  })
})
