/**
 * 私有仓前端模块的装配缝。
 *
 * `import.meta.glob` 自动发现 `modules/<id>/index.ts`，所以私有仓只要新增文件，
 * 不用改任何共享文件（改了就会和 upstream 冲突）。
 *
 * 开源仓里这个目录是空的，`adminModules` 就是空数组，行为完全不变。
 */
export interface AccountWidget {
  id: string
  /** 挂到哪个插槽：目前只有 `billing`（/me/billing 页）。 */
  slot: string
  component: () => Promise<unknown>
}

export interface AdminModule {
  id: string
  /** 追加到路由表（由 `routes.ts` 合并）。 */
  routes?: unknown[]
  /** 追加到语言包。 */
  messages?: Record<string, unknown>
  /** 追加到登录页的登录方式。 */
  loginMethods?: unknown[]
  /** 注入到个人中心页面的卡片（如充值卡）。 */
  accountWidgets?: AccountWidget[]
}

const discovered = import.meta.glob('./*/index.ts', { eager: true }) as Record<
  string,
  { default?: AdminModule }
>

export const adminModules: AdminModule[] = Object.values(discovered)
  .map((m) => m.default)
  .filter((m): m is AdminModule => Boolean(m && m.id))

/** 某个插槽下所有模块贡献的组件。 */
export function widgetsFor(slot: string): AccountWidget[] {
  return adminModules.flatMap((m) => m.accountWidgets ?? []).filter((w) => w.slot === slot)
}
