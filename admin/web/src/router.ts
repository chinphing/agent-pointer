import { createRouter, createWebHistory } from 'vue-router'
import { authGuard } from './guard'
import { ROUTES } from './routes'

// history 路由：`/me/billing` 这类多段路径 hash 模式做不到（服务端拿不到 `#` 之后的内容）。
// 刷新不 404 由服务端 fallback 兜底（见 `admin/src/main.rs` 的 `spa_or_404`）。
// `BASE_URL` 让子路径部署（`https://host/pointer/`）也能正常工作。
export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: ROUTES.map((route) => ({
    path: route.path,
    component: route.component,
    meta: {
      title: route.titleKey,
      public: route.public,
      perm: route.perm,
      search: route.search,
      section: route.section,
    },
  })),
})

router.beforeEach(authGuard)
