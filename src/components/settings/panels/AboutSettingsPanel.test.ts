// @vitest-environment node
//
// 「检查更新」入口曾经挂在构建期口味标记 `VITE_POINTER_EDITION` 上：
//
//   const showUpdater = isManagedEdition() && isTauriRuntime()
//
// 于是变成两个开关必须同时打开——企业构建漏设那个变量，更新功能其实是好的，
// 但「关于」页里入口凭空消失，从界面上看不出有更新，很难查。
//
// 现在只认运行时绑定：`isStandalone` 由 `get_auth_mode` 从
// `control_plane_bound()` 推出，和 `useAppUpdater` 的门禁是同一个判据。
//
// 挂载整个面板要拖进 Tauri invoke、更新器与设置 store，这里在 SFC 层面钉住。
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import * as platformUrls from '../../../../src/lib/platformUrls'

const sfc = readFileSync(new URL('./AboutSettingsPanel.vue', import.meta.url), 'utf8')

describe('关于页的更新入口', () => {
  it('只按运行时绑定判定，不再看构建期口味', () => {
    expect(sfc).toContain('isTauriRuntime() && !platformAuth.isStandalone')
    expect(sfc).not.toContain('isManagedEdition')
    expect(sfc).not.toContain('VITE_POINTER_EDITION')
  })

  it('构建期口味标记已从工具模块移除', () => {
    expect('isManagedEdition' in platformUrls).toBe(false)
    expect('pointerEdition' in platformUrls).toBe(false)
  })
})
