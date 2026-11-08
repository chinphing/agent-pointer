<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
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

/** 账号菜单挂在顶栏右上角（设计图 02/03/04/08/09 都是这样）。 */
const menuOpen = ref(false)

/** 顶栏标题取自路由 meta，页面正文不再重复写一遍 h1。 */
const pageTitle = computed(() => (route.meta.title ? t(String(route.meta.title)) : ''))

/**
 * 顶栏搜索：设计图上并非每页都有 ——
 *   概览「搜索成员 / 资源」、资源页「搜索资源名称」、角色页与档位映射页没有搜索框。
 * 用路由 meta.search 决定有没有、占位写什么、回车往哪儿带。
 */
const searchKind = computed(() => (route.meta.search as string) || '')
const search = ref('')

const searchPlaceholder = computed(() => {
  if (searchKind.value === 'resource') return t('topbar.searchResource')
  if (searchKind.value === 'member') return t('topbar.searchMember')
  return t('topbar.searchGlobal')
})

function submitSearch() {
  const q = search.value.trim()
  if (!q) return
  // 概览是跨页搜索，落到资源列表；其它页原地筛。
  const target =
    searchKind.value === 'member' ? '/members' : '/resources'
  const samePage = route.path === target
  if (samePage) router.replace({ path: target, query: { q } })
  else router.push({ path: target, query: { q } })
}

/** 换页时清空输入，避免上一页的关键词留在框里误导。 */
watch(
  () => route.path,
  () => {
    search.value = ''
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
        <span class="avatar">{{ avatarText }}</span>
        <span class="account-text">
          <span class="account-name">{{ session.user.displayName }}</span>
          <span class="account-role">{{ roleNames.join(' / ') || t('app.noRole') }}</span>
        </span>
      </div>
    </aside>

    <div class="content">
      <header class="topbar">
        <h1>{{ pageTitle }}</h1>
        <div class="topbar-right">
          <input
            v-if="searchKind"
            v-model="search"
            class="search"
            :placeholder="searchPlaceholder"
            :title="t('topbar.searchHint')"
            @keyup.enter="submitSearch"
          />
          <div class="account-wrap">
            <button class="avatar-btn" :class="{ open: menuOpen }" @click="menuOpen = !menuOpen">
              <span class="avatar">{{ avatarText }}</span>
            </button>
            <div v-if="menuOpen" class="account-menu">
              <div class="who">
                <div class="account-name">{{ session.user.displayName }}</div>
                <div class="account-role">{{ roleNames.join(' / ') || t('app.noRole') }}</div>
              </div>
              <div class="menu-divider"></div>
              <div class="lang-row">
                <button :class="{ active: locale === 'zh-CN' }" @click="switchLocale('zh-CN')">中文</button>
                <button :class="{ active: locale === 'en' }" @click="switchLocale('en')">English</button>
              </div>
              <div class="menu-divider"></div>
              <button class="logout" @click="doLogout">{{ t('app.logout') }}</button>
            </div>
          </div>
        </div>
      </header>

      <main class="main">
        <router-view />
      </main>
    </div>
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

.side-foot {
  margin-top: auto;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 7px 8px;
}

/* 设计图 02：底部只有圆头像 + 名字 + 角色，纯展示，不带控件 */
.account { display: flex; align-items: center; gap: 10px; }

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
.content { flex: 1; min-width: 0; display: flex; flex-direction: column; }

/* 顶栏（设计图 02/03/04/08/09）：白底 56px，标题在左，搜索与头像在右 */
.topbar {
  height: 56px;
  flex: 0 0 56px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  padding: 0 24px;
  background: var(--panel);
  border-bottom: 1px solid var(--line);
  position: sticky;
  top: 0;
  z-index: 15;
}
.topbar h1 { margin: 0; font-size: 15px; font-weight: 600; }
.topbar-right { display: flex; align-items: center; gap: 12px; }

.topbar .search {
  width: 180px;
  height: 28px;
  padding: 0 10px;
  border-radius: 8px;
  border: 1px solid transparent;
  background: var(--panel-alt);
  font-size: 12px;
}
.topbar .search:focus { border-color: var(--accent); background: var(--panel); outline: none; }

.account-wrap { position: relative; }
.avatar-btn { border: none; background: transparent; padding: 0; cursor: pointer; display: flex; }
.avatar-btn .avatar { background: #e8e8eb; }
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
  z-index: 20;
}
.account-menu .who { padding: 2px 4px; display: flex; flex-direction: column; gap: 1px; }
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

.main { flex: 1; padding: 22px 24px 60px; max-width: 1180px; width: 100%; box-sizing: border-box; }
</style>
