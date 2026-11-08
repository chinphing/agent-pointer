<script setup lang="ts">
import { computed, onMounted } from 'vue'
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

function switchLocale(e: Event) {
  setLocale((e.target as HTMLSelectElement).value as AdminLocale)
}

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
        <div class="small">{{ session.user.displayName }}</div>
        <div class="small muted">{{ roleNames.join(' / ') || t('app.noRole') }}</div>
        <select class="tiny lang" :value="locale" @change="switchLocale">
          <option value="zh-CN">中文</option>
          <option value="en">English</option>
        </select>
        <button class="tiny" @click="doLogout">{{ t('app.logout') }}</button>
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
  width: 208px;
  flex: 0 0 208px;
  background: var(--panel-alt);
  border-right: 1px solid var(--line);
  padding: 20px 14px;
  display: flex;
  flex-direction: column;
}

.brand { display: flex; gap: 10px; align-items: center; padding: 0 6px 18px; }
.dot {
  width: 10px; height: 10px; border-radius: 50%;
  background: var(--accent); flex: 0 0 10px;
}

nav { display: flex; flex-direction: column; gap: 2px; }

.nav-item {
  padding: 8px 12px;
  border-radius: 8px;
  color: var(--fg);
  font-size: 14px;
}
.nav-item:hover { background: var(--line); }
.nav-item.active { background: var(--accent-soft); color: var(--accent); font-weight: 600; }

.side-foot { margin-top: auto; padding: 12px 6px 0; display: flex; flex-direction: column; gap: 4px; }
.side-foot button { margin-top: 6px; }
.side-foot .lang { margin-top: 6px; }

.main { flex: 1; padding: 26px 30px 60px; max-width: 1180px; }
</style>
