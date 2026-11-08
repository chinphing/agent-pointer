<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { api, errText } from '../api'
import { fmtCount as fmt, when } from '../lib/format'

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

/** 近 7 天柱状图的基准高度用最大值，全 0 时给 1 避免除零。 */
const trendMax = computed(() => {
  const list: any[] = data.value?.tokenTrend || []
  return Math.max(1, ...list.map((d: any) => Number(d.tokens || 0)))
})

const ACTION_LABEL: Record<string, string> = {
  login: '登录',
  login_failed: '登录失败',
  'user.create': '创建账号',
  'user.import': '批量导入',
  'user.roles': '改角色',
  'user.status': '改账号状态',
  'user.password': '重置密码',
  'resource.create': '创建资源',
  'resource.update': '修改资源',
  'resource.delete': '删除资源',
  'resource.grants': '改授权',
  'resource.reveal': '查看密钥',
  'skill.version.upload': '上传版本',
  'skill.version.publish': '发布版本',
  'usage.quota': '改配额',
}

function actionText(action: string) {
  return ACTION_LABEL[action] || action
}

const resourceTotal = computed(() => {
  const map = data.value?.resources || {}
  return Object.values(map).reduce((sum: number, n: any) => sum + Number(n || 0), 0)
})

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
      <h2>近 7 天 Token 用量</h2>
      <div v-if="!data.tokenTrend?.length" class="empty">还没有用量数据</div>
      <div v-else class="trend">
        <div v-for="d in data.tokenTrend" :key="d.day" class="bar-wrap">
          <div class="bar-col">
            <div
              class="bar"
              :style="{ height: Math.max(2, (Number(d.tokens) / trendMax) * 100) + '%' }"
              :title="`${d.day}：${fmt(d.tokens)} tokens`"
            ></div>
          </div>
          <div class="small muted">{{ String(d.day).slice(5) }}</div>
        </div>
      </div>
    </div>

    <div class="panel">
      <h2>最近操作</h2>
      <div v-if="!data.recentActions?.length" class="empty">还没有操作记录</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th style="width: 170px">时间</th><th style="width: 120px">操作人</th><th>操作</th></tr>
        </thead>
        <tbody>
          <tr v-for="(a, i) in data.recentActions" :key="i">
            <td class="small muted">{{ when(a.createdAt) }}</td>
            <td class="small">{{ a.actorName || '—' }}</td>
            <td class="small">
              {{ actionText(a.action) }}
              <span v-if="a.targetType" class="muted">· {{ a.targetType }}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="panel">
      <h2>资源分布</h2>
      <div v-if="!Object.keys(data.resources || {}).length" class="empty">还没有任何资源</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th>类型</th><th class="num">数量</th></tr>
        </thead>
        <tbody>
          <tr v-for="(count, kind) in data.resources" :key="kind">
            <td>{{ KIND_LABEL[String(kind)] || kind }}</td>
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

.trend { display: flex; gap: 10px; align-items: flex-end; margin-top: 14px; }
.bar-wrap { flex: 1; text-align: center; }
.bar-col { height: 110px; display: flex; align-items: flex-end; }
.bar {
  width: 100%;
  background: var(--accent);
  border-radius: 6px 6px 2px 2px;
  min-height: 2px;
}
</style>
