import { createRouter, createWebHashHistory } from 'vue-router'
import { authGuard } from './guard'

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
    { path: '/overview', meta: { title: 'nav.overview', search: 'global' }, component: Overview },
    { path: '/members', meta: { title: 'nav.members', search: 'member' }, component: Members },
    { path: '/roles', meta: { title: 'nav.roles' }, component: Roles },
    { path: '/resources', meta: { title: 'nav.resources', search: 'resource' }, component: Resources },
    { path: '/releases', meta: { title: 'nav.releases' }, component: () => import('./pages/Releases.vue') },
    { path: '/usage', meta: { title: 'nav.usage' }, component: Usage },
    { path: '/audit', meta: { title: 'nav.audit' }, component: Audit },
    { path: '/:pathMatch(.*)*', redirect: '/overview' },
  ],
})

router.beforeEach(authGuard)
