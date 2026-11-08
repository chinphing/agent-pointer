<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { session, loadSession, logout } from './store'
import { roleLabel } from './lib/roles'
import { setLocale, type AdminLocale } from './i18n'

const route = useRoute()
const router = useRouter()
const { t, locale } = useI18n()

onMounted(async () => {
  await loadSession()
  if (!session.user && route.path !== '/login') router.replace('/login')
})

const nav = [
  { path: '/overview', key: 'nav.overview', perm: 'system:read' },
  { path: '/members', key: 'nav.members', perm: 'user:read' },
  { path: '/roles', key: 'nav.roles', perm: 'role:read' },
  { path: '/resources', key: 'nav.resources', perm: '' },
  { path: '/usage', key: 'nav.usage', perm: 'usage:read' },
  { path: '/audit', key: 'nav.audit', perm: 'audit:read' },
]

/** 角色一律走统一解析：内置角色用 i18n，自定义角色用管理员起的名字。 */
const roleNames = computed(() =>
  (session.roles || []).map((r: any) => {
    const key = typeof r === 'string' ? r : r.key
    const name = typeof r === 'string' ? '' : r.name
    return roleLabel(key, name, t)
  })
)

/** 账号菜单：语言与退出登录收在这里，侧栏底部保持设计图那样只有头像+名字+角色。 */
const menuOpen = ref(false)

function switchLocale(next: AdminLocale) {
  setLocale(next)
  menuOpen.value = false
}

/** 头像里的字：取显示名首字，没有就用账号首字母。 */
const avatarText = computed(() => {
  const name = (session.user?.displayName || session.user?.username || '?').trim()
  return name.slice(0, 1).toUpperCase()
})

function visible(perm: string) {
  if (!perm) return true
  return session.permissions.includes(perm)
}

async function doLogout() {
  await logout()
  router.replace('/login')
}
</script>

<template>
  <div v-if="!session.ready" class="boot muted">{{ t('app.loading') }}</div>

  <router-view v-else-if="!session.user" />

  <div v-else class="shell">
    <aside class="side">
      <div class="brand">
        <span class="dot"></span>
        <div>
          <strong>Pointer</strong>
          <div class="small muted">{{ t('app.subtitle') }}</div>
        </div>
      </div>

      <nav>
        <router-link
          v-for="item in nav"
          :key="item.path"
          v-show="visible(item.perm)"
          :to="item.path"
          class="nav-item"
          active-class="active"
        >
          {{ t(item.key) }}
        </router-link>
      </nav>

      <div class="side-foot">
        <button class="account" :class="{ open: menuOpen }" @click="menuOpen = !menuOpen">
          <span class="avatar">{{ avatarText }}</span>
          <span class="account-text">
            <span class="account-name">{{ session.user.displayName }}</span>
            <span class="account-role">{{ roleNames.join(' / ') || t('app.noRole') }}</span>
          </span>
          <span class="account-caret">⋯</span>
        </button>

        <div v-if="menuOpen" class="account-menu">
          <div class="lang-row">
            <button :class="{ active: locale === 'zh-CN' }" @click="switchLocale('zh-CN')">中文</button>
            <button :class="{ active: locale === 'en' }" @click="switchLocale('en')">English</button>
          </div>
          <div class="menu-divider"></div>
          <button class="logout" @click="doLogout">{{ t('app.logout') }}</button>
        </div>
      </div>
    </aside>

    <main class="main">
      <router-view />
    </main>
  </div>
</template>

<style scoped>
.boot { padding: 60px; text-align: center; }

.shell { display: flex; min-height: 100vh; }

.side {
  width: 212px;
  flex: 0 0 212px;
  /* 吸顶 + 自身滚动：页面再长，左下角的账号区也不会被顶到视口外 */
  position: sticky;
  top: 0;
  align-self: flex-start;
  height: 100vh;
  box-sizing: border-box;
  background: var(--panel-alt);
  border-right: 1px solid var(--line);
  padding: 20px 12px;
  display: flex;
  flex-direction: column;
}

.brand { display: flex; gap: 10px; align-items: center; padding: 0 6px 18px; }
.dot {
  width: 10px; height: 10px; border-radius: 50%;
  background: var(--accent); flex: 0 0 10px;
}

nav { display: flex; flex-direction: column; gap: 2px; overflow-y: auto; }

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

.side-foot { margin-top: auto; position: relative; padding: 0 0 8px; }

/* 设计图 02：圆头像 + 名字 + 角色，一行，没有别的控件 */
.account {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 7px;
  border: none;
  background: transparent;
  border-radius: 8px;
  cursor: pointer;
  text-align: left;
}
.account:hover, .account.open { background: #e8e8eb; }

.avatar {
  width: 26px; height: 26px; flex: 0 0 26px;
  border-radius: 50%;
  background: #d4d4d8;
  color: #52525b;
  font-size: 11px;
  font-weight: 600;
  display: flex; align-items: center; justify-content: center;
}

.account-text { display: flex; flex-direction: column; gap: 1px; min-width: 0; flex: 1; }
.account-name { font-size: 12px; color: var(--fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.account-role { font-size: 10px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.account-caret { color: var(--muted); font-size: 13px; line-height: 1; opacity: 0; }
.account:hover .account-caret, .account.open .account-caret { opacity: 1; }

.account-menu {
  position: absolute;
  left: 0; right: 0; bottom: 46px;
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 10px;
  box-shadow: 0 8px 24px rgb(0 0 0 / 8%);
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  z-index: 20;
}
.account-menu .lang-row { display: flex; gap: 4px; }
.account-menu .lang-row button {
  flex: 1;
  font-size: 12px;
  padding: 5px 6px;
  border-radius: 6px;
}
.account-menu .lang-row button.active {
  background: var(--accent-soft);
  color: var(--accent);
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

.main { flex: 1; padding: 26px 30px 60px; max-width: 1180px; }
</style>
