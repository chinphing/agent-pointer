<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api } from '../../api'
import { when } from '../../lib/format'
import { fmtCompact } from '../../lib/chart'
import BarChart from '../../components/BarChart.vue'
import RechargeDialog from '../../components/RechargeDialog.vue'
import { ledgerKindLabel } from '../../lib/ledger'

const { t } = useI18n()

/**
 * 我的账户 = 「我是谁 + 我花了多少」。
 * 账单 = 完整流水；消费明细 = 逐条调用。这里只放概览，不重复列全量数据。
 */
const loading = ref(true)
const error = ref('')
const balanceYuan = ref('0.00')

/**
 * 充值做成弹窗，不是独立页面：充值只是账户页上的一个动作，付完就地看到余额变化。
 * 支付能力不是每个部署都有（开源版没有支付后端），所以先探测再决定要不要出按钮。
 */
const payEnabled = ref(false)
const payChannel = ref('')
const showRecharge = ref(false)

interface LedgerRow {
  kind: string
  amountYuan: string
  balanceYuan: string
  model: string
  note: string
  createdAt: number
}

interface DayPoint {
  day: string
  totalTokens: number
}

const ledger = ref<LedgerRow[]>([])
const spentYuan = ref('0.00')
const todaySpentYuan = ref('0.00')
const calls = ref(0)
const monthTokens = ref(0)
const byDay = ref<DayPoint[]>([])

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/** 最近 7 天，缺的日子补 0，柱子数量固定，不会因没数据而抖动。 */
const weekBars = computed(() => {
  const map = new Map(byDay.value.map((d) => [d.day, d.totalTokens]))
  const out: { day: string; label: string; tokens: number }[] = []
  const now = new Date()
  for (let i = 6; i >= 0; i--) {
    const d = new Date(now)
    d.setDate(now.getDate() - i)
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
      d.getDate()
    ).padStart(2, '0')}`
    out.push({ day: key, label: key.slice(5), tokens: map.get(key) ?? 0 })
  }
  return out
})

const hasUsage = computed(() => byDay.value.some((d) => d.totalTokens > 0))

/** 概览里只展示最近 3 条，全量去账单页看。 */
const recent = computed(() => ledger.value.slice(0, 3))

/** 余额与最近流水。充值成功后要能单独刷新，所以从 onMounted 里抽出来。 */
async function loadWallet() {
  try {
    const w = await api.get('/api/admin/my-wallet')
    balanceYuan.value = w.balanceYuan ?? '0.00'
    ledger.value = w.ledger ?? []
  } catch (e) {
    error.value = errText(e)
  }
}

/** 到账后刷新余额，不用用户自己刷页面。 */
function onPaid() {
  error.value = ''
  void loadWallet()
}

onMounted(async () => {
  // 支付能力探测失败不报错：开源版没有支付后端，按钮不出现即可。
  api
    .get('/api/admin/pay/enabled')
    .then((r) => {
      payEnabled.value = r.enabled === true
      payChannel.value = r.channel || ''
    })
    .catch(() => {
      payEnabled.value = false
    })

  try {
    // 余额与流水、用量统计是两个接口，任一失败都不该让整页白屏。
    const [wallet, usage] = await Promise.allSettled([
      api.get('/api/admin/my-wallet'),
      api.get('/api/admin/my-usage'),
    ])
    if (wallet.status === 'fulfilled') {
      balanceYuan.value = wallet.value.balanceYuan ?? '0.00'
      ledger.value = wallet.value.ledger ?? []
    } else {
      error.value = errText(wallet.reason)
    }
    if (usage.status === 'fulfilled') {
      const u = usage.value
      spentYuan.value = u.spentYuan ?? '0.00'
      todaySpentYuan.value = u.todaySpentYuan ?? '0.00'
      calls.value = u.calls ?? 0
      monthTokens.value = u.totals?.totalTokens ?? 0
      byDay.value = u.byDay ?? []
    }
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="me-page">
    <div v-if="error" class="err">{{ error }}</div>

    <!-- 余额 + 快捷入口 -->
    <section class="me-card">
      <div class="me-balance-row">
        <div>
          <div class="me-balance-label">{{ t('me.balance') }}</div>
          <div class="me-balance">
            <span v-if="loading" class="muted" style="font-size: 15px">{{ t('common.loading') }}</span>
            <template v-else>¥{{ balanceYuan }}</template>
          </div>
        </div>
        <div class="row" style="gap: 10px">
          <button v-if="payEnabled" class="btn primary" type="button" @click="showRecharge = true">
            {{ t('me.recharge') }}
          </button>
          <router-link to="/me/billing" class="btn">{{ t('me.viewBilling') }}</router-link>
          <router-link to="/me/usage" class="btn">{{ t('me.viewUsage') }}</router-link>
        </div>
      </div>
    </section>

    <RechargeDialog
      v-if="showRecharge"
      :channel="payChannel"
      @close="showRecharge = false"
      @paid="onPaid"
    />

    <!-- 本月概览 -->
    <section class="me-card">
      <div class="me-card-head">
        <div>
          <div class="me-card-title">{{ t('me.overview') }}</div>
          <div class="me-card-sub">{{ t('me.overviewHint') }}</div>
        </div>
      </div>

      <div class="me-stats">
        <div class="me-stat">
          <div class="me-stat-value">¥{{ spentYuan }}</div>
          <div class="me-stat-label">{{ t('me.monthSpent') }}</div>
        </div>
        <div class="me-stat">
          <div class="me-stat-value">¥{{ todaySpentYuan }}</div>
          <div class="me-stat-label">{{ t('me.todaySpent') }}</div>
        </div>
        <div class="me-stat">
          <div class="me-stat-value">{{ fmtCompact(monthTokens) }}</div>
          <div class="me-stat-label">{{ t('me.monthTokens') }}</div>
        </div>
        <div class="me-stat">
          <div class="me-stat-value">{{ calls }}</div>
          <div class="me-stat-label">{{ t('me.calls') }}</div>
        </div>
      </div>

      <div class="me-chart">
        <div class="me-chart-head">
          <span class="small muted">{{ t('me.last7d') }}</span>
          <span v-if="!hasUsage" class="small muted">{{ t('me.noUsageYet') }}</span>
        </div>
        <BarChart
          :height="120"
          :points="weekBars.map((b) => ({ label: b.label, value: b.tokens, hint: `${b.day}：${fmtCompact(b.tokens)} tokens` }))"
        />
      </div>
    </section>

    <!-- 最近流水 -->
    <section class="me-card">
      <div class="me-card-head">
        <div class="me-card-title">{{ t('me.recentLedger') }}</div>
        <router-link to="/me/billing" class="small">{{ t('me.viewAll') }}</router-link>
      </div>

      <div v-if="loading" class="empty">{{ t('common.loading') }}</div>
      <div v-else-if="!recent.length" class="empty">{{ t('me.emptyLedger') }}</div>
      <ul v-else class="me-recent">
        <li v-for="(row, i) in recent" :key="i" class="me-recent-row">
          <span class="muted small">{{ when(row.createdAt) }}</span>
          <span class="small">{{ ledgerKindLabel(row.kind, t) }}</span>
          <span class="mono small" :class="row.kind === 'charge' ? 'me-minus' : 'me-plus'">
            {{ row.kind === 'charge' ? '−' : '+' }}¥{{ row.amountYuan.replace('-', '') }}
          </span>
          <span class="mono small muted">{{ t('me.balanceAfterShort') }} ¥{{ row.balanceYuan }}</span>
        </li>
      </ul>
    </section>
  </div>
</template>

<style scoped>
/* 近 7 天柱状图：纯 CSS，不引图表库 */
.me-chart { margin-top: 22px; }
.me-chart-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 10px; }

/* 最近流水 */
.me-recent { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
.me-recent-row {
  display: grid;
  grid-template-columns: 150px 64px 1fr 120px;
  align-items: center;
  gap: 12px;
  padding: 12px 0;
  border-bottom: 1px solid var(--line);
}
/* 金额列右对齐，跟余额列贴在一起，视线不用横跨整张卡片 */
.me-recent-row > :nth-child(3) { text-align: right; }
.me-recent-row > :nth-child(4) { text-align: right; }
.me-recent-row:last-child { border-bottom: none; padding-bottom: 0; }
.me-minus { color: var(--danger); }
.me-plus { color: var(--ok); }
</style>
