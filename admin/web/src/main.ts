import { createApp } from 'vue'
import App from './App.vue'
import { router } from './router'
import { i18n } from './i18n'
import './styles.css'

const app = createApp(App).use(i18n).use(router)

// 等首次路由解析完再挂载：否则 App.vue 的 onMounted 里 `route.fullPath` 还是 `/`，
// 未登录时带出去的 `?next=` 会变成 `?next=/`，登录后回不到用户真正要去的页面。
router.isReady().then(() => app.mount('#app'))
