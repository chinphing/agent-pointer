<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'
import { fmtCount as fmt, when } from '../lib/format'

const loading = ref(true)
const error = ref('')
const { t } = useI18n()
const data = ref<any>(null)
const usage = ref<any>(null)

const KIND_LABEL: Record<string, string> = {
  provider: t('kind.provider'),
  model: t('kind.model'),
  skill: t('kind.skill'),
  plugin: t('kind.plugin'),
  mcp: 'MCP',
  tier_profile: t('kind.tier_profile'),
}

/** 近 7 天柱状图的基准高度用最大值，全 0 时给 1 避免除零。 */
const trendMax = computed(() => {
  const list: any[] = data.value?.tokenTrend || []
  return Math.max(1, ...list.map((d: any) => Number(d.tokens || 0)))
})

function actionText(action: string) {
  const key = `action.${action}`
  return t(key) === key ? action : t(key)
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
  <h1>{{ t('overview.title') }}</h1>
  <p class="muted small" style="margin-top: 4px">{{ t('overview.subtitle') }}</p>

  <div v-if="error" class="err" style="margin-top: 16px">{{ error }}</div>
  <div v-else-if="loading" class="empty">{{ t('common.loading') }}</div>

  <template v-else-if="data">
    <div v-if="!data.oauthConfigured" class="err" style="margin-top: 16px">
      尚未配置 <span class="mono">POINTER_ADMIN_OAUTH_SECRET</span>，客户端无法登录换取凭证。
    </div>

    <div class="grid" style="margin-top: 16px">
      <div class="panel stat">
        <div class="muted small">账号</div>
        <div class="big">{{ data.users.total }}</div>
        <div class="small muted">{{ t('overview.usersActive', { n: data.users.active }) }}</div>
      </div>
      <div class="panel stat">
        <div class="muted small">{{ t('overview.resourceTotal') }}</div>
        <div class="big">{{ resourceTotal }}</div>
        <div class="small muted">{{ t('overview.kinds', { n: Object.keys(data.resources || {}).length }) }}</div>
      </div>
      <div class="panel stat">
        <div class="muted small">{{ t('overview.todayTokens') }}</div>
        <div class="big">{{ fmt(data.todayTokens) }}</div>
        <div class="small muted">{{ t('overview.monthTokens', { n: fmt(usage?.totals?.total || 0) }) }}</div>
      </div>
    </div>

    <div class="panel" style="margin-top: 16px">
      <h2>{{ t('overview.last7d') }}</h2>
      <div v-if="!data.tokenTrend?.length" class="empty">{{ t('overview.noUsageData') }}</div>
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
      <h2>{{ t('overview.recent') }}</h2>
      <div v-if="!data.recentActions?.length" class="empty">{{ t('overview.noActions') }}</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th style="width: 170px">{{ t('overview.time') }}</th><th style="width: 120px">{{ t('overview.actor') }}</th><th>{{ t('overview.action') }}</th></tr>
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
      <h2>{{ t('overview.resourceDist') }}</h2>
      <div v-if="!Object.keys(data.resources || {}).length" class="empty">{{ t('overview.noResources') }}</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr><th>{{ t('overview.type') }}</th><th class="num">{{ t('overview.count') }}</th></tr>
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
      <h2>{{ t('overview.monthByModel') }}</h2>
      <div v-if="!usage?.byModel?.length" class="empty">{{ t('overview.noUsage') }}</div>
      <table v-else style="margin-top: 10px">
        <thead>
          <tr>
            <th>{{ t('usage.model') }}</th>
            <th class="num">{{ t('usage.input') }}</th>
            <th class="num">{{ t('usage.output') }}</th>
            <th class="num">{{ t('usage.total') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in usage.byModel" :key="row.key">
            <td class="mono">{{ row.key || t('overview.unlabeled') }}</td>
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
