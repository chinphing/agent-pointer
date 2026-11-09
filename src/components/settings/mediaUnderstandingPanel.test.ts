// @vitest-environment node
//
// 媒体理解曾经是「3 类型 × 3 档位」：设置页每类给一个 快速/标准/高级 单选，
// 再弹窗配三套模型。档位只有在「每轮可切换」时才有价值——智能体三档在输入框
// 旁能切，媒体理解没有这个入口，于是配了 9 个槽、实际只用 3 个。
//
// 收敛成「类型 → 单模型」后，界面必须跟着变；否则会出现「界面还在选档位、
// 下发只有一份」的错配。挂载整个面板要拖进 Tauri invoke 和设置 store，
// 这里在 SFC 层面钉住。
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

const assistant = readFileSync(
  new URL('./panels/AssistantSettingsPanel.vue', import.meta.url),
  'utf8'
)
const sceneModal = readFileSync(new URL('./SceneTierModelsModal.vue', import.meta.url), 'utf8')
const debugPanel = readFileSync(
  new URL('./panels/AgentSettingsPanel.vue', import.meta.url),
  'utf8'
)

describe('媒体理解的设置界面', () => {
  it('助手面板不再给媒体类型配档位单选', () => {
    // 媒体区块里不该再出现档位单选或本地档位状态。
    expect(assistant).not.toContain('mediaUnderstandingModesLocal')
    expect(assistant).not.toContain('mediaUnderstandingModes')
    // 每类显示当前模型名（单模型）。
    expect(assistant).toContain('mediaModelLabel')
    expect(assistant).toContain('mediaModelLabel(row.key)')
    expect(assistant).toContain('mediaUnderstanding(kind as')
  })

  it('档位弹窗的媒体分支只渲染一行', () => {
    // 媒体分支不再 v-for 档位；智能体分支保留 modeTiers。
    expect(sceneModal).not.toContain('form.mediaModeLlm')
    expect(sceneModal).not.toContain('selectMediaModeModel')
    expect(sceneModal).toContain('form.mediaUnderstanding(mediaKind)')
    expect(sceneModal).toContain('selectMediaUnderstandingModel')
    // 智能体那条分支还在用档位。
    expect(sceneModal).toContain('modeTiers')
  })

  it('调试面板的媒体表格按类型一行，不再按档位展开', () => {
    expect(debugPanel).not.toContain('mediaModeLlm')
    expect(debugPanel).not.toContain('selectMediaModeModel')
    expect(debugPanel).toContain('mediaUnderstanding(kind)')
    expect(debugPanel).toContain('selectMediaUnderstandingModel')
  })

  it('旧键名在整个前端源码里已经不存在', () => {
    const src = readFileSync(new URL('../../types/chat.ts', import.meta.url), 'utf8')
    expect(src).not.toContain('MediaModeLlmMap')
    expect(src).not.toContain('MediaUnderstandingModes')
    expect(src).toContain('MediaUnderstandingModels')
  })
})
