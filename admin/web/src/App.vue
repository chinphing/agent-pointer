<script setup lang="ts">
import { onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { session, loadSession, logout } from './store'

const route = useRoute()
const router = useRouter()

onMounted(async () => {
  await loadSession()
  if (!session.user && route.path !== '/login') router.replace('/login')
})

const nav = [
  { path: '/overview', label: '概览', perm: 'system:read' },
  { path: '/members', label: '成员', perm: 'user:read' },
  { path: '/roles', label: '角色', perm: 'role:read' },
  { path: '/resources', label: '资源', perm: '' },
  { path: '/usage', label: '用量', perm: 'usage:read' },
  { path: '/audit', label: '审计', perm: 'audit:read' },
]

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
  <div v-if="!session.ready" class="boot muted">加载中…</div>

  <router-view v-else-if="!session.user" />

  <div v-else class="shell">
    <aside class="side">
      <div class="brand">
        <span class="dot"></span>
        <div>
          <strong>Pointer</strong>
          <div class="small muted">企业控制面</div>
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
          {{ item.label }}
        </router-link>
      </nav>

      <div class="side-foot">
        <div class="small">{{ session.user.displayName }}</div>
        <div class="small muted">{{ session.roles.join('、') || '无角色' }}</div>
        <button class="tiny" @click="doLogout">退出登录</button>
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

.main { flex: 1; padding: 26px 30px 60px; max-width: 1180px; }
</style>
