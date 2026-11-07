<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { api, errText } from '../api'

const loading = ref(true)
const error = ref('')
const data = ref<any>(null)
const usage = ref<any>(null)

const KIND_LABEL: Record<string, string> = {
  provider: '模型服务',
  model: '模型',
  skill: '技能',
  plugin: '插件',
  mcp: 'MCP',
  tier_profile: '档位映射',
}

const resourceTotal = computed(() => {
  const map = data.value?.resources || {}
  return Object.values(map).reduce((sum: number, n: any) => sum + Number(n || 0), 0)
})

function fmt(n: number): string {
  return Number(n || 0).toLocaleString('zh-CN')
}

onMounted(async () => {
  try {
    const [overview, current] = await Promise.all([
      api.get('/api/admin/overview'),
      api.get('/api/admin/usage'),
    ])
    data.value = overview
    usage.value = current
  } catch (e) {
    error.value = errText(e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <h1>概览</h1>
  <p class="muted small" style="margin-top: 4px">账号、资源与本月消耗。</p>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-else-if="loading" class="empty">加载中…</div>

  <template v-else-if="data">
    <div v-if="!data.oauthConfigured" class="err" style="margin-top: 16px">
      尚未配置 <span class="mono">POINTER_ADMIN_OAUTH_SECRET</span>，客户端无法登录换取凭证。
    </div>

    <div class="grid" style="margin-top: 16px">
      <div class="panel stat">
        <div class="muted small">账号</div>
        <div class="big">{{ data.users.total }}</div>
        <div class="small muted">其中启用 {{ data.users.active }}</div>
      </div>
      <div class="panel stat">
        <div class="muted small">资源总数</div>
        <div class="big">{{ resourceTotal }}</div>
        <div class="small muted">{{ Object.keys(data.resources || {}).length }} 类</div>
      </div>
      <div class="panel stat">
        <div class="muted small">今日 tokens</div>
        <div class="big">{{ fmt(data.todayTokens) }}</div>
        <div class="small muted">本月 {{ fmt(usage?.totals?.total || 0) }}</div>
      </div>
    </div>

    <div class="panel" style="margin-top: 16px">
      <h2>资源分布</h2>
      <div v-if="!Object.keys(data.resources || {}).length" class="empty">还没有任何资源</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th>类型</th><th class="num">数量</th></tr>
        </thead>
        <tbody>
          <tr v-for="(count, kind) in data.resources" :key="kind">
            <td>{{ KIND_LABEL[kind as string] || kind }}</td>
            <td class="num">{{ count }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="panel">
      <h2>本月按模型</h2>
      <div v-if="!usage?.byModel?.length" class="empty">本月还没有用量上报</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr>
            <th>模型</th>
            <th class="num">输入</th>
            <th class="num">输出</th>
            <th class="num">合计</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in usage.byModel" :key="row.key">
            <td class="mono">{{ row.key || '（未标注）' }}</td>
            <td class="num">{{ fmt(row.promptTokens) }}</td>
            <td class="num">{{ fmt(row.completionTokens) }}</td>
            <td class="num">{{ fmt(row.total) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </template>
</template>

<style scoped>
.grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
.stat .big { font-size: 26px; font-weight: 600; margin: 6px 0 2px; }
</style>
