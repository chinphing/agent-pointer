// @vitest-environment node
//
// 桌面委派三档必须挂在**桌面自动化**分区里，和「电脑行为」并列。
//
// 踩过两次坑：
//   1. 先写进 `AgentSettingsPanel.vue` —— 那个组件早已没人挂载，界面上根本没有入口；
//   2. 再塞进智能体卡片（`AssistantSettingsPanel.vue`）—— 那里是每个智能体的档位/模型，
//      塞一个全局开关进去，既不符合该卡片的信息结构，也和已有的「桌面自动化」分区重复。
//
// 挂载整个面板要拖进 Tauri invoke 和设置 store，这里在 SFC 源码层面钉住位置。
import { describe, expect, it } from 'vitest'
import { existsSync, readFileSync } from 'node:fs'

const dialog = readFileSync(new URL('./SettingsDialog.vue', import.meta.url), 'utf8')
const desktop = readFileSync(
  new URL('./panels/GenerationSettingsPanel.vue', import.meta.url),
  'utf8'
)
const assistant = readFileSync(
  new URL('./panels/AssistantSettingsPanel.vue', import.meta.url),
  'utf8'
)
const form = readFileSync(
  new URL('../../composables/useSettingsDialogForm.ts', import.meta.url),
  'utf8'
)

describe('桌面委派三档的设置入口', () => {
  it('挂在设置弹窗真正渲染的「生成/系统」面板里', () => {
    expect(dialog).toContain(
      "import GenerationSettingsPanel from './panels/GenerationSettingsPanel.vue'"
    )
    expect(dialog).toContain('<GenerationSettingsPanel :form="form" />')
  })

  it('位于「桌面自动化」分区，与「电脑行为」卡片并列', () => {
    const desktopSection = desktop.slice(
      desktop.indexOf('system-desktop-heading'),
      desktop.indexOf('system-media-gen-heading')
    )
    expect(desktopSection).toContain('settings.desktop.delegation')
    expect(desktopSection).toContain('name="computer-delegation"')
    // 委派卡片排在「电脑行为」之前：先决定能不能派，再谈派出去之后的行为。
    expect(desktopSection.indexOf('settings.desktop.delegation'))
      .toBeLessThan(desktopSection.indexOf('settings.desktop.computerBehavior'))
  })

  it('三档都有单选，且选中档有说明文案', () => {
    expect(desktop).toContain('settings.desktop.delegationOff')
    expect(desktop).toContain('settings.desktop.delegationConfirm')
    expect(desktop).toContain('settings.desktop.delegationAuto')
    expect(desktop).toContain('computerDelegationHint')
  })

  it('不再出现在智能体卡片里（那是每个智能体的档位/模型，不是全局开关）', () => {
    expect(assistant).not.toContain('computerDelegation')
  })

  it('没人挂载的 AgentSettingsPanel 已删除，不会被重新接回来', () => {
    expect(existsSync(new URL('./panels/AgentSettingsPanel.vue', import.meta.url))).toBe(false)
  })

  it('表单把值接进了保存与回填链路', () => {
    expect(form).toContain("const computerDelegation = ref<ComputerDelegation>('confirm')")
    expect(form).toContain('computerDelegation.value = s.settings.computerDelegation')
    expect(form).toContain('computerDelegation: computerDelegation.value')
    expect(form).toContain('s.settings.computerDelegation = payload.computerDelegation')
    expect(form).toContain('s.userSettings.computerDelegation = payload.computerDelegation')
  })

  it('中英文案都齐', () => {
    for (const path of ['../../locales/zh-CN.json', '../../locales/en.json']) {
      const raw = readFileSync(new URL(path, import.meta.url), 'utf8')
      for (const key of [
        'delegation',
        'delegationHint',
        'delegationTier',
        'delegationOff',
        'delegationOffHint',
        'delegationConfirm',
        'delegationConfirmHint',
        'delegationAuto',
        'delegationAutoHint'
      ]) {
        expect(raw).toContain(`"${key}"`)
      }
    }
  })
})
