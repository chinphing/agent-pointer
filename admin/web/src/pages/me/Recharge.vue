<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../../api'

const { t } = useI18n()

/**
 * 充值页。
 *
 * 支付能力不是每个部署都有（开源版没有支付后端），所以先探测
 * `/api/admin/pay/enabled`：探测不到就整页显示「未开通」，不显示表单。
 *
 * 二维码由**服务端**渲染成 SVG，前端只放一个 `<img>`——这样开源仓不必为了
 * 一个私有功能引入二维码库。
 */

interface Order {
  id: string
  amountMicro: number
  status: string
  codeUrl: string | null
}

const PRESETS = [10, 50, 100, 200]
const POLL_MS = 2000
const POLL_MAX = 150 // 5 分钟

const enabled = ref(false)
const channel = ref('')
const probed = ref(false)
const amountYuan = ref(50)
const custom = ref('')
const busy = ref(false)
const error = ref('')
const order = ref<Order | null>(null)
const paid = ref(false)
const timedOut = ref(false)

let timer: ReturnType<typeof setInterval> | null = null
let ticks = 0

const amountMicro = computed(() => {
  const yuan = custom.value.trim() ? Number(custom.value) : amountYuan.value
  if (!Number.isFinite(yuan) || yuan <= 0) return 0
  // 元 → micro 整数，避免浮点误差
  return Math.round(yuan * 1_000_000)
})

onMounted(async () => {
  try {
    const r = await api.get('/api/admin/pay/enabled')
    enabled.value = r.enabled === true
    channel.value = r.channel || ''
  } catch {
    enabled.value = false
  } finally {
    probed.value = true
  }
})

onUnmounted(stopPoll)

function stopPoll() {
  if (timer) {
    clearInterval(timer)
    timer = null
  }
}

async function createOrder() {
  error.value = ''
  if (amountMicro.value <= 0) {
    error.value = t('recharge.invalidAmount')
    return
  }
  busy.value = true
  try {
    const r = await api.post('/api/admin/pay/orders', { amountMicro: amountMicro.value })
    order.value = r.order as Order
    paid.value = false
    timedOut.value = false
    startPoll()
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

/** 轮询订单状态。回调可能丢，所以这里也会顺手触发一次服务端查单。 */
function startPoll() {
  stopPoll()
  ticks = 0
  timer = setInterval(async () => {
    if (!order.value) return stopPoll()
    ticks += 1
    if (ticks > POLL_MAX) {
      stopPoll()
      timedOut.value = true
      return
    }
    try {
      const fresh = (await api.get(`/api/admin/pay/orders/${order.value.id}`)) as Order
      order.value = fresh
      if (fresh.status === 'paid') {
        paid.value = true
        stopPoll()
      } else if (fresh.status === 'closed' || fresh.status === 'failed') {
        stopPoll()
      }
    } catch {
      // 轮询失败不弹错：下一轮再试，避免网络抖动刷屏
    }
  }, POLL_MS)
}

/** mock 模式下没有真实扫码，给一个「模拟支付」按钮把流程走完。 */
async function mockPay() {
  if (!order.value) return
  busy.value = true
  try {
    await api.post(`/api/admin/pay/orders/${order.value.id}/mock-pay`, {})
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

async function syncOrder() {
  if (!order.value) return
  busy.value = true
  try {
    const r = await api.post(`/api/admin/pay/orders/${order.value.id}/sync`, {})
    order.value = r.order as Order
    if (order.value.status === 'paid') {
      paid.value = true
      stopPoll()
    }
  } catch (e) {
    error.value = errText(e)
  } finally {
    busy.value = false
  }
}

function reset() {
  stopPoll()
  order.value = null
  paid.value = false
  timedOut.value = false
  error.value = ''
}
</script>

<template>
  <div v-if="!probed" class="muted">{{ t('common.loading') }}</div>

  <div v-else-if="!enabled" class="card muted">{{ t('recharge.disabled') }}</div>

  <div v-else class="recharge">
    <div v-if="error" class="err">{{ error }}</div>

    <!-- 还没下单：选金额 -->
    <template v-if="!order">
      <p class="muted">{{ t('recharge.subtitle') }}</p>

      <label class="label">{{ t('recharge.amount') }}</label>
      <div class="presets">
        <button
          v-for="p in PRESETS"
          :key="p"
          type="button"
          class="preset"
          :class="{ active: !custom.trim() && amountYuan === p }"
          @click="amountYuan = p; custom = ''"
        >
          {{ p }} {{ t('recharge.yuan') }}
        </button>
      </div>

      <label class="label" for="custom">{{ t('recharge.custom') }}</label>
      <input id="custom" v-model="custom" inputmode="decimal" :placeholder="t('recharge.customHint')" />

      <button class="btn primary" style="margin-top: 18px" :disabled="busy" @click="createOrder">
        {{ busy ? t('recharge.submitting') : t('recharge.submit') }}
      </button>
    </template>

    <!-- 已下单：二维码 + 状态 -->
    <template v-else>
      <div class="pay-box">
        <img
          v-if="!paid"
          :src="`/api/admin/pay/orders/${order.id}/qr`"
          :alt="t('recharge.scanHint')"
          width="240"
          height="240"
        />
        <div v-if="!paid" class="muted small">{{ t('recharge.scanHint') }}</div>

        <div v-if="paid" class="ok">{{ t('recharge.paid') }}</div>
        <div v-else-if="order.status === 'closed'" class="muted">{{ t('recharge.closed') }}</div>
        <div v-else-if="timedOut" class="muted">{{ t('recharge.expired') }}</div>
        <div v-else class="muted">{{ t('recharge.pending') }}</div>

        <div class="amount-line">
          {{ (order.amountMicro / 1_000_000).toFixed(2) }} {{ t('recharge.yuan') }}
        </div>
      </div>

      <div class="actions">
        <button v-if="channel === 'mock' && !paid" class="btn" :disabled="busy" @click="mockPay">
          {{ t('recharge.mockPay') }}
        </button>
        <button v-if="!paid && order.status === 'pending'" class="btn" :disabled="busy" @click="syncOrder">
          {{ t('recharge.iPaid') }}
        </button>
        <button class="btn" @click="reset">{{ t('recharge.again') }}</button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.recharge {
  max-width: 520px;
}
.card {
  padding: 20px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--panel);
}
.label {
  display: block;
  margin: 16px 0 6px;
  font-size: 13px;
  color: var(--muted);
}
.presets {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.preset {
  padding: 8px 16px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: #fff;
  font-size: 14px;
  cursor: pointer;
}
.preset.active {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-soft);
}
.pay-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 20px;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--panel);
}
.pay-box img {
  display: block;
  border-radius: 8px;
}
.amount-line {
  font-size: 20px;
  font-weight: 600;
}
.ok {
  color: var(--ok);
  font-weight: 600;
}
.actions {
  display: flex;
  gap: 10px;
  margin-top: 16px;
}
</style>
