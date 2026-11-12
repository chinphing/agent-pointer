/**
 * ELK 布局的桩。
 *
 * mermaid 12 把 elkjs 变成了硬依赖（1.4 MB，占前端 JS 的 26%），但 Pointer 不支持
 * elk 布局：渲染前 `downgradeElkLayout` 会把 `layout: elk` 改写成 dagre，elk 代码
 * 根本不会被加载（见 `vite.config.ts` 的 resolve.alias）。
 *
 * 留这个桩是为了万一有漏网的请求，报错能指回这里，而不是一个含糊的
 * "elkjs is not a function"。
 */
export default class ElkLayoutStub {
  layout(): Promise<never> {
    return Promise.reject(
      new Error('[pointer] ELK layout is not bundled in this build; diagrams render with dagre')
    )
  }
}
