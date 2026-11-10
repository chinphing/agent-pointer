import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import { authGuard } from './guard'
import { ROUTES } from './routes'

// history 路由：`/me/billing` 这类多段路径 hash 模式做不到（服务端拿不到 `#` 之后的内容）。
// 刷新不 404 由服务端 fallback 兜底（见 `admin/src/main.rs` 的 `spa_or_404`）。
// `BASE_URL` 让子路径部署（`https://host/pointer/`）也能正常工作。
export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: ROUTES.map((route): RouteRecordRaw => {
    const meta = {
      title: route.titleKey,
      public: route.public,
      perm: route.perm,
      search: route.search,
      section: route.section,
    }
    // 重定向路由没有组件：两者同时给 vue-router 会报「不能既是 redirect 又有 component」。
    if (route.redirect) return { path: route.path, redirect: route.redirect, meta }
    // 非重定向路由的 component 一定存在（路由表是静态的），这里断言只是为了过类型。
    return { path: route.path, component: route.component!, meta }
  }),
})

router.beforeEach(authGuard)
