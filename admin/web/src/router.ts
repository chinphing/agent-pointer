import { createRouter, createWebHashHistory } from 'vue-router'
import { session } from './store'

import Overview from './pages/Overview.vue'
import Members from './pages/Members.vue'
import Roles from './pages/Roles.vue'
import Resources from './pages/Resources.vue'
import Usage from './pages/Usage.vue'
import Audit from './pages/Audit.vue'
import Login from './pages/Login.vue'

// hash 路由：控制面是单进程静态托管，刷新任意页面都不会 404
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/overview' },
    { path: '/login', component: Login, meta: { public: true } },
    { path: '/overview', component: Overview },
    { path: '/members', component: Members },
    { path: '/roles', component: Roles },
    { path: '/resources', component: Resources },
    { path: '/usage', component: Usage },
    { path: '/audit', component: Audit },
    { path: '/:pathMatch(.*)*', redirect: '/overview' },
  ],
})

router.beforeEach((to) => {
  if (to.meta.public) return true
  if (!session.ready) return true
  if (!session.user) return { path: '/login' }
  return true
})
