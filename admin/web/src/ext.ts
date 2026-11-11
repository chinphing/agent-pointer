import type { Component } from 'vue'

/**
 * 前端扩展点 —— 与后端 `admin/src/ext/mod.rs` 的 `Ext` trait 对称。
 *
 * 开源版这里全是空的。支付、短信这类只属于私有部署的能力，界面代码放在私有仓，
 * 构建时用 vite alias 把 `@ext` 指过去（见 `vite.config.ts`）。开源仓因此不含
 * 任何私有功能的业务代码与文案。
 *
 * 私有仓要加界面，只改自己那份 `ext.ts`，不用动开源仓。
 */

/**
 * 账号页余额旁的动作按钮，例如「充值」。点开渲染 `component` 指向的弹窗。
 *
 * 弹窗组件约定两个事件：
 * - `close` —— 用户关掉了弹窗
 * - `done`  —— 操作成功、页面数据该刷新了
 */
export interface AccountAction {
  /** 唯一标识，用作渲染 key。 */
  key: string
  /** 按钮文案的 i18n key。文案由扩展通过 `messages` 提供。 */
  labelKey: string
  /** 异步加载的弹窗组件。 */
  component: () => Promise<{ default: Component }>
  /** 返回 false 就不显示这个按钮（例如该部署没开通这项能力）。 */
  available?: () => Promise<boolean>
}

/** 登录页「账号密码」之外的方式，例如短信登录。 */
export interface LoginMethod {
  key: string
  /** 切换按钮的文案 i18n key。 */
  labelKey: string
  /** 异步加载的表单组件，登录成功后 `emit('success')`。 */
  component: () => Promise<{ default: Component }>
  available?: () => Promise<boolean>
}

/**
 * 合并进 i18n 的文案，形状同 `locales/zh-CN.json`。
 *
 * 例：`{ 'zh-CN': { foo: { title: '标题' } }, en: { foo: { title: 'Title' } } }`
 */
export const messages: Record<string, Record<string, unknown>> = {}

/**
 * 流水类型的额外标签：后端 `kind` 的值 → i18n key。
 * 开源版只有 grant / charge，其余由扩展补充。
 */
export const ledgerKindKeys: Record<string, string> = {}

/** 账号页的动作按钮。开源版为空。 */
export const ACCOUNT_ACTIONS: AccountAction[] = []

/** 额外的登录方式。开源版为空。 */
export const LOGIN_METHODS: LoginMethod[] = []
