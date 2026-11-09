// @vitest-environment node
//
// 插件面板的两个列表必须**并列**渲染：企业提供（按需安装）和本地已安装。
//
// 曾经把它们串成 v-if / v-else-if / v-else 一条链，结果是：企业只要下发了一个
// 还没安装的插件，员工本地已装的那几个就整段不渲染——看起来像「自带插件消失了」。
// 挂载整个面板要拖进 Tauri invoke 与设置 store，这里在 SFC 层面钉住结构。
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

const sfc = readFileSync(new URL('./PluginsPanel.vue', import.meta.url), 'utf8')

/** 取出某个标记之后、到下一个空行为止的一段，便于做局部断言。 */
function around(marker: string, lines = 8): string {
  const i = sfc.indexOf(marker)
  expect(i, `找不到标记：${marker}`).toBeGreaterThan(-1)
  return sfc.slice(i, i + 400).split('\n').slice(0, lines).join('\n')
}

describe('PluginsPanel 列表条件', () => {
  it('企业提供区用独立的 v-if', () => {
    expect(sfc).toContain('v-if="orgPlugins.length"')
  })

  it('本地插件列表不能被串成 v-else', () => {
    // 列表块必须有自己的 v-if，而不是承接上面那个 v-if 的 v-else。
    expect(sfc).toContain('v-if="plugins.length" class="space-y-3"')
    expect(sfc).not.toContain('v-else class="space-y-3"')
    expect(sfc).not.toContain('v-else-if="plugins.length === 0"')
  })

  it('空状态只在两边都空时才出现', () => {
    const empty = around('plugins.length === 0 && !orgPlugins.length')
    expect(empty).toContain('settings.plugins.empty')
  })

  it('企业区排在本地列表之前（顺序不影响渲染，但影响可读性）', () => {
    expect(sfc.indexOf('v-if="orgPlugins.length"')).toBeLessThan(
      sfc.indexOf('v-if="plugins.length" class="space-y-3"')
    )
  })
})
