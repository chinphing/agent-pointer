<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { session, loadSession, logout } from './store'
import { SECTIONS, tabsFor, topNavFor, type Section } from './routes'
import { authGuard } from './guard'
import { roleLabel } from './lib/roles'
import { setLocale, type AdminLocale } from './i18n'

const route = useRoute()
const router = useRouter()
const { t, locale } = useI18n()

onMounted(async () => {
  await loadSession()
  // 会话就绪后按同一套规则再判一次当前路由：首屏守卫会因为 `!session.ready` 放行，
  // 这里补上判定，同时**不能**把公开页（首页 / 下载）也踢到登录页。
  const verdict = authGuard({ meta: route.meta })
  if (verdict !== true) router.replace(verdict.path)
})

/** 当前所在板块。登录页、404 这类不属于任何板块，顶部导航不高亮任何一项。 */
const currentSection = computed(() => route.meta.section as Section | undefined)

/** 顶部导航四项；未登录只剩首页与下载，零权限看不到控制面板。 */
const topNav = computed(() => topNavFor(session.permissions, Boolean(session.user)))

/** 只有个人中心与控制面板有左侧栏，内容取自各自板块的页面表。 */
const sidebarItems = computed(() => {
  const section = currentSection.value
  if (!section) return []
  const def = SECTIONS.find((s) => s.key === section)
  if (!def?.hasSidebar) return []
  return tabsFor(section, session.permissions)
})

/** 角色一律走统一解析：内置角色用 i18n，自定义角色用管理员起的名字。 */
const roleNames = computed(() =>
  (session.roles || []).map((r: any) => {
    const key = typeof r === 'string' ? r : r.key
    const name = typeof r === 'string' ? '' : r.name
    return roleLabel(key, name, t)
  })
)

const menuOpen = ref(false)

/** 页面标题取自路由 meta，页面正文不再重复写一遍 h1。 */
/** 换页时清空输入，避免上一页的关键词留在框里误导。 */
watch(
  () => route.path,
  () => {
    menuOpen.value = false
  }
)

function switchLocale(next: AdminLocale) {
  setLocale(next)
  menuOpen.value = false
}

/** 头像里的字：取显示名首字，没有就用账号首字母。 */
const avatarText = computed(() => {
  const name = (session.user?.displayName || session.user?.username || '?').trim()
  return name.slice(0, 1).toUpperCase()
})

async function doLogout() {
  await logout()
  router.replace('/login')
}
</script>

<template>
  <div v-if="!session.ready" class="boot muted">{{ t('app.loading') }}</div>

  <template v-else>
    <!-- 顶部导航：四个板块。个人中心与控制面板的侧边栏在下面，不占这里的位置。 -->
    <header class="topnav">
      <router-link to="/" class="brand">
        <span class="dot"></span>
        <strong>Pointer</strong>
      </router-link>

      <div class="topnav-right">
        <nav class="sections">
          <router-link
            v-for="item in topNav"
            :key="item.key"
            :to="item.path"
            class="section-link"
            :class="{ active: currentSection === item.key }"
          >
            {{ t(item.labelKey) }}
          </router-link>
        </nav>

        <router-link v-if="!session.user" to="/login" class="login-btn">
          {{ t('nav.login') }}
        </router-link>
        <div v-else class="account-wrap">
          <button class="avatar-btn" :class="{ open: menuOpen }" @click="menuOpen = !menuOpen">
            <span class="avatar">{{ avatarText }}</span>
          </button>
          <div v-if="menuOpen" class="account-menu">
            <div class="who">
              <div class="who-text">
                <div class="account-name">{{ session.user.displayName }}</div>
                <div class="account-role">{{ roleNames.join(' / ') || t('app.noRole') }}</div>
              </div>
              <!-- 语言切换：紧凑双态开关，不再单占一整行 -->
              <div class="lang-switch" role="group" :aria-label="t('app.language')">
                <button
                  :class="{ active: locale === 'zh-CN' }"
                  :aria-pressed="locale === 'zh-CN'"
                  @click="switchLocale('zh-CN')"
                >
                  中
                </button>
                <button
                  :class="{ active: locale === 'en' }"
                  :aria-pressed="locale === 'en'"
                  @click="switchLocale('en')"
                >
                  EN
                </button>
              </div>
            </div>
            <div class="menu-divider"></div>
            <button class="logout" @click="doLogout">{{ t('app.logout') }}</button>
          </div>
        </div>
      </div>
    </header>

    <div class="shell">
      <aside v-if="sidebarItems.length" class="side">
        <nav>
          <router-link
            v-for="item in sidebarItems"
            :key="item.path"
            :to="item.path"
            class="nav-item"
            active-class="active"
          >
            {{ t(item.titleKey) }}
          </router-link>
        </nav>

        <div class="side-foot">
          <span class="avatar">{{ avatarText }}</span>
          <span class="account-text">
            <span class="account-name">{{ session.user?.displayName }}</span>
            <span class="account-role">{{ roleNames.join(' / ') || t('app.noRole') }}</span>
          </span>
        </div>
      </aside>

      <div class="content">
        <main class="main">
          <router-view />
        </main>
      </div>
    </div>
  </template>
</template>

<style scoped>
.boot { padding: 60px; text-align: center; }

/* 顶部导航：四个板块横排，品牌在左，账号在右 */
.topnav {
  height: 56px;
  display: flex;
  align-items: center;
  gap: 28px;
  padding: 0 24px;
  background: var(--panel);
  border-bottom: 1px solid var(--line);
  position: sticky;
  top: 0;
  z-index: 30;
}

.brand { display: flex; gap: 9px; align-items: center; color: var(--fg); }
.brand strong { font-size: 15px; }
.dot {
  width: 10px; height: 10px; border-radius: 50%;
  background: var(--accent); flex: 0 0 10px;
}

.sections { display: flex; align-items: center; gap: 4px; }
.section-link {
  height: 32px;
  display: flex;
  align-items: center;
  padding: 0 12px;
  border-radius: 8px;
  color: var(--fg);
  font-size: 14px;
  white-space: nowrap;
}
.section-link:hover { background: var(--panel-alt); }
.section-link.active { background: var(--accent-soft); color: var(--accent); font-weight: 600; }

.topnav-right { margin-left: auto; display: flex; align-items: center; gap: 16px; }

.login-btn {
  height: 30px;
  display: flex;
  align-items: center;
  padding: 0 14px;
  border-radius: 999px;
  background: var(--accent);
  color: #fff;
  font-size: 13px;
}

.account-wrap { position: relative; }
.avatar-btn { border: none; background: transparent; padding: 0; cursor: pointer; display: flex; }
.avatar {
  width: 28px; height: 28px; flex: 0 0 28px;
  border-radius: 50%;
  background: #e8e8eb;
  color: #52525b;
  font-size: 11px;
  font-weight: 600;
  display: flex; align-items: center; justify-content: center;
}
.avatar-btn.open .avatar, .avatar-btn:hover .avatar { background: #d4d4d8; }

.account-menu {
  position: absolute;
  right: 0;
  top: 38px;
  width: 200px;
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 10px;
  box-shadow: 0 8px 24px rgb(0 0 0 / 8%);
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  z-index: 40;
}
.account-menu .who {
  padding: 2px 4px;
  display: flex;
  align-items: center;
  gap: 10px;
}
.account-menu .who-text { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 1px; }

/* 语言切换：两个 22px 小格拼成一个开关，宽度约 52px */
.lang-switch {
  display: flex;
  flex: none;
  border: 1px solid var(--line);
  border-radius: 6px;
  overflow: hidden;
}
.lang-switch button {
  border: 0;
  border-radius: 0;
  background: transparent;
  color: var(--muted);
  font-size: 11px;
  line-height: 1;
  padding: 4px 7px;
}
.lang-switch button + button { border-left: 1px solid var(--line); }
.lang-switch button.active {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
  border-color: transparent;
}
.menu-divider { height: 1px; background: var(--line); margin: 2px 4px; }
.account-menu .logout {
  font-size: 12px;
  padding: 6px 8px;
  border: none;
  background: transparent;
  border-radius: 6px;
  text-align: left;
  color: var(--muted);
}
.account-menu .logout:hover { background: var(--panel-alt); color: var(--danger); }

/* 侧边栏只属于个人中心 / 控制面板 */
.shell { display: flex; min-height: calc(100vh - 56px); }

.side {
  width: 200px;
  flex: 0 0 200px;
  /* 吸顶位置让开顶部导航；自身滚动，左下角账号区不会被顶出视口 */
  position: sticky;
  top: 56px;
  align-self: flex-start;
  height: calc(100vh - 56px);
  box-sizing: border-box;
  background: var(--panel-alt);
  border-right: 1px solid var(--line);
  padding: 16px 12px;
  display: flex;
  flex-direction: column;
}

.side nav { display: flex; flex-direction: column; gap: 2px; overflow-y: auto; }

.nav-item {
  height: 32px;
  display: flex;
  align-items: center;
  padding: 0 12px;
  border-radius: 8px;
  color: var(--fg);
  font-size: 14px;
}
.nav-item:hover { background: var(--line); }
.nav-item.active { background: var(--accent-soft); color: var(--accent); font-weight: 600; }

.side-foot {
  margin-top: auto;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 7px 8px;
}
.account-text { display: flex; flex-direction: column; gap: 1px; min-width: 0; flex: 1; }
.account-name { font-size: 12px; color: var(--fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.account-role { font-size: 10px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.content { flex: 1; min-width: 0; display: flex; flex-direction: column; }

.main { flex: 1; padding: 36px 24px 60px; max-width: 1180px; width: 100%; box-sizing: border-box; margin: 0 auto; }
</style>
