<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, errText } from '../api'

const items = ref<any[]>([])
const allPermissions = ref<string[]>([])
const loading = ref(true)
const error = ref('')

const ROLE_DESC: Record<string, string> = {
  super_admin: '隐式拥有全部权限，不能通过界面缩减',
  admin: '日常管理，但看不到明文密钥、不能改角色定义',
  viewer: '只读：能看成员、资源、用量与审计',
  member: '零管理端权限，只是普通员工（登录客户端用）',
}

onMounted(async () => {
  try {
    const [roleList, perms] = await Promise.all([
      api.get('/api/admin/roles'),
      api.get('/api/admin/permissions'),
    ])
    items.value = roleList.items
    allPermissions.value = perms.items
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <h1>角色</h1>
  <p class="muted small" style="margin-top: 4px">
    内置角色是代码常量，M0 不提供自定义角色；权限判定取并集，没有 deny。
  </p>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-else-if="loading" class="empty">加载中…</div>

  <template v-else>
    <div v-for="role in items" :key="role.key" class="panel" style="margin-top: 16px">
      <div class="spread">
        <div>
          <h2>
            {{ role.name }}
            <span class="tag" style="margin-left: 6px">{{ role.key }}</span>
          </h2>
          <p class="muted small" style="margin: 4px 0 0">{{ ROLE_DESC[role.key] || '' }}</p>
        </div>
        <div class="small muted">{{ role.memberCount }} 人</div>
      </div>

      <div class="row wrap" style="margin-top: 12px; gap: 6px">
        <span v-for="p in role.permissions" :key="p" class="tag plain mono">{{ p }}</span>
        <span v-if="!role.permissions.length" class="muted small">无管理端权限</span>
      </div>
    </div>

    <div class="panel">
      <h2>全部权限（{{ allPermissions.length }}）</h2>
      <div class="row wrap" style="margin-top: 10px; gap: 6px">
        <span v-for="p in allPermissions" :key="p" class="tag plain mono">{{ p }}</span>
      </div>
    </div>
  </template>
</template>
