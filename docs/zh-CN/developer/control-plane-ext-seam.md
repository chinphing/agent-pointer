# 控制面扩展缝
[English](../../en/developer/control-plane-ext-seam.md) | 简体中文

开源版的控制面（`admin/`）是完整的，但**不是每个部署都具备全部能力**。支付充值、
短信登录这类能力只存在于私有部署，它们的代码不能进开源仓——否则开源仓会夹带用不到
的业务代码，改私有功能也要动公开仓。

扩展缝就是这条边界：**开源仓只留槽位，私有仓填内容**。后端一个 trait，前端一个模块。

## 1. 后端：`Ext` trait

实现位置 `admin/src/ext/mod.rs`。

```rust
pub trait Ext: Send + Sync {
    fn extra_permissions(&self) -> Vec<Permission> { vec![] }
    fn router(&self, state: &Shared) -> Router { Router::new() }
    fn public_router(&self) -> Router { Router::new() }
    fn migrations(&self) -> Vec<(i64, &'static str)> { vec![] }
}
```

`install()` 把实现放进 `OnceLock`，`main.rs` 在启动时装配一次：

```rust
ext::install(Box::new(EnterpriseExt::new(config)))?;   // 开源版是 NoExt
migrations::migrate(&store).await?;                     // 含扩展的表
let app = app::build_app(state);                        // 含扩展的路由
```

四条缝的用途：

| 方法 | 用途 |
|---|---|
| `extra_permissions` | 扩展自己的权限键，进权限矩阵 |
| `router` | 需要登录的路由（`/api/admin/pay/*`） |
| `public_router` | 不需要登录的路由（`/api/pay/notify` 回调、`/api/auth/sms/*`） |
| `migrations` | 扩展自己的表，版本号从 9000 起，避开开源仓的号段 |

`migrations` 用独立号段是有意的：开源仓的版本号会一直往上涨，扩展用 9xxx
永远不会撞上，升级时互不干扰。

## 2. 前端：`@ext` 模块

实现位置 `admin/web/src/ext.ts`，开源版**全部为空**。

```ts
export interface AccountAction {          // 账号页余额旁的动作按钮
  key: string
  labelKey: string                        // i18n key，文案由扩展提供
  component: () => Promise<{ default: Component }>
  available?: () => Promise<boolean>      // false 就不显示按钮
}

export interface LoginMethod {            // 登录页「账号密码」之外的方式
  key: string
  labelKey: string
  component: () => Promise<{ default: Component }>
  available?: () => Promise<boolean>
}

export const messages = {}                // 合并进 i18n
export const ledgerKindKeys = {}          // 流水类型 kind → i18n key
export const ACCOUNT_ACTIONS = []
export const LOGIN_METHODS = []
```

四个消费点：

| 位置 | 行为 |
|---|---|
| `pages/me/Index.vue` | 渲染 `ACCOUNT_ACTIONS`，点开异步加载弹窗 |
| `pages/Login.vue` | 渲染 `LOGIN_METHODS`，一种时用右上角切换，多种时用 tab |
| `i18n.ts` | `messages` 深合并进语言包，扩展只写自己那几段 |
| `lib/ledger.ts` | `ledgerKindKeys` 并入类型标签；未知 kind 仍原样显示 |

**约定**：动作/表单组件 `emit('close')` 表示关闭，`emit('done')`（动作）或
`emit('success')`（登录）表示成功。宿主页面据此刷新数据或跳转。

`available` 存在的理由：能力是运行时决定的，不是构建时。私有部署没开支付就不该
出现充值按钮——宁可不显示，也不显示一个点进去报错的入口。

## 3. 私有仓怎么接

私有仓**不复制开源前端代码**，只加自己的模块，构建时把 `@ext` 换掉：

```ts
// 私有仓 web/vite.config.ts
const upstream = '../upstream/admin/web'

export default defineConfig({
  root: upstream,                    // 直接构建开源仓的前端
  resolve: {
    alias: {
      '@ext': './ext.ts',            // 换成私有实现
      '@app': `${upstream}/src`,     // 复用接口封装、store、类型
    },
    // 运行时库必须只有一份，否则响应式和 i18n 各走各的
    dedupe: ['vue', 'vue-i18n', 'vue-router'],
  },
  build: { outDir: './dist' },
})
```

私有仓的 `ext.ts` 长这样：

```ts
export const ACCOUNT_ACTIONS = [{
  key: 'recharge',
  labelKey: 'me.recharge',
  available: probePay,
  component: () => import('./RechargeDialog.vue'),
}]
```

部署时 `POINTER_ADMIN_STATIC_DIR` 指向**私有仓的 `dist`**，不是开源仓的——
后者里根本没有支付与短信界面。

## 4. 约束

- **开源仓不得出现扩展的业务代码与文案**。要加界面，改私有仓的 `ext.ts`。
- **扩展不复制开源代码**。用 `@app` 引开源仓的模块，不要拷贝一份。
- **能力探测放扩展里**。开源仓不知道「支付」「短信」这些概念，它只问
  `available()` 返回什么。
- 弹窗组件别依赖宿主传 props：动作由 `ACCOUNT_ACTIONS` 里的 `component` 自行加载，
  需要的信息由扩展自己的模块缓存（例如能力探测时顺便记住渠道）。

## 5. 怎么验证边界没破

```bash
cd admin/web && npm run build
grep -rl "pay/orders\|sms/login\|账户充值" dist/assets/*.js   # 必须无输出
```

开源构建产物里出现扩展的接口路径或文案，就说明有东西漏进了开源仓。
