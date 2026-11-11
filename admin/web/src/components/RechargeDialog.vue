<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, errText } from '../api'

/**
 * 充值对话框。
 *
 * 以前是侧边栏里的独立页面，但充值只是账户页上的一个动作，不是目的地——
 * 独立页要来回跳转，付完还得自己回去看余额。改成弹窗：点按钮就在原地完成，
 * 成功后账户页的余额自动刷新。
 *
 * 二维码由**服务端**渲染成 SVG，前端只放一个 `<img>`——这样开源仓不必为了
 * 一个私有功能引入二维码库。
 */
const props = defineProps<{ channel: string }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'paid'): void }>()

const { t } = useI18n()

interface Order {
  id: string
  amountMicro: number
  status: string
  codeUrl: string | null
}

const PRESETS = [10, 50, 100, 200]
const POLL_MS = 2000
const POLL_MAX = 150 // 5 分钟

const amountYuan = ref(50)
const custom = ref('')
const busy = ref(false)
const error = ref('')
const order = ref<Order | null>(null)
const paid = ref(false)
const timedOut = ref(false)

let timer: ReturnType<typeof setInterval> | null = null
let ticks = 0
let prevOverflow = ''

const amountMicro = computed(() => {
  const yuan = custom.value.trim() ? Number(custom.value) : amountYuan.value
  if (!Number.isFinite(yuan) || yuan <= 0) return 0
  // 元 → micro 整数，避免浮点误差
  return Math.round(yuan * 1_000_000)
})

const amountLabel = computed(() =>
  order.value ? (order.value.amountMicro / 1_000_000).toFixed(2) : ''
)

onMounted(() => {
  // 弹窗打开时锁住背后的页面滚动，否则滚轮会把底下的内容滚走
  prevOverflow = document.body.style.overflow
  document.body.style.overflow = 'hidden'
  window.addEventListener('keydown', onKey)
})

onUnmounted(() => {
  stopPoll()
  document.body.style.overflow = prevOverflow
  window.removeEventListener('keydown', onKey)
})

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

function stopPoll() {
  if (timer) {
    clearInterval(timer)
    timer = null
  }
}

function close() {
  stopPoll()
  emit('close')
}

function pick(yuan: number) {
  amountYuan.value = yuan
  custom.value = ''
  error.value = ''
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
      if (fresh.status === 'paid') markPaid()
      else if (fresh.status === 'closed' || fresh.status === 'failed') stopPoll()
    } catch {
      // 轮询失败不弹错：下一轮再试，避免网络抖动刷屏
    }
  }, POLL_MS)
}

/** 到账只通知一次，避免轮询和手动查单各触发一遍余额刷新。 */
function markPaid() {
  if (paid.value) return
  paid.value = true
  stopPoll()
  emit('paid')
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
    if (order.value.status === 'paid') markPaid()
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
  <div class="modal-mask" @click.self="close">
    <div
      class="modal recharge-modal"
      role="dialog"
      aria-modal="true"
      :aria-label="t('recharge.title')"
    >
      <div class="dialog-head">
        <div>
          <h2>{{ t('recharge.title') }}</h2>
          <p v-if="!order" class="muted small sub">{{ t('recharge.subtitle') }}</p>
        </div>
        <button class="dialog-x" type="button" :aria-label="t('common.close')" @click="close">
          ×
        </button>
      </div>

      <div v-if="error" class="err">{{ error }}</div>

      <!-- 还没下单：选金额 -->
      <template v-if="!order">
        <label class="label">{{ t('recharge.amount') }}</label>
        <div class="presets">
          <button
            v-for="p in PRESETS"
            :key="p"
            type="button"
            class="preset"
            :class="{ active: !custom.trim() && amountYuan === p }"
            @click="pick(p)"
          >
            <span class="preset-num">{{ p }}</span>
            <span class="preset-unit">{{ t('recharge.yuan') }}</span>
          </button>
        </div>

        <label class="label" for="recharge-custom">{{ t('recharge.custom') }}</label>
        <div class="input-wrap">
          <span class="prefix">¥</span>
          <input
            id="recharge-custom"
            v-model="custom"
            inputmode="decimal"
            :placeholder="t('recharge.customHint')"
          />
        </div>
      </template>

      <!-- 已下单：二维码 + 状态 -->
      <div v-else class="pay-box">
        <img
          v-if="!paid"
          :src="`/api/admin/pay/orders/${order.id}/qr`"
          :alt="t('recharge.scanHint')"
          width="240"
          height="240"
        />
        <div v-if="paid" class="ok">✓ {{ t('recharge.paid') }}</div>
        <div v-else-if="order.status === 'closed'" class="muted">{{ t('recharge.closed') }}</div>
        <div v-else-if="timedOut" class="muted">{{ t('recharge.expired') }}</div>
        <div v-else class="muted small">{{ t('recharge.scanHint') }}</div>

        <div class="amount-line">
          ¥{{ amountLabel }}
          <span v-if="!paid && order.status === 'pending' && !timedOut" class="waiting">
            {{ t('recharge.pending') }}
          </span>
        </div>
      </div>

      <div class="modal-actions">
        <template v-if="!order">
          <button class="btn" type="button" @click="close">{{ t('common.cancel') }}</button>
          <button class="btn primary" type="button" :disabled="busy" @click="createOrder">
            {{ busy ? t('recharge.submitting') : t('recharge.submit') }}
          </button>
        </template>
        <template v-else>
          <button v-if="!paid" class="btn" type="button" @click="reset">
            {{ t('recharge.again') }}
          </button>
          <button
            v-if="channel === 'mock' && !paid"
            class="btn"
            type="button"
            :disabled="busy"
            @click="mockPay"
          >
            {{ t('recharge.mockPay') }}
          </button>
          <button
            v-if="!paid && order.status === 'pending' && !timedOut"
            class="btn"
            type="button"
            :disabled="busy"
            @click="syncOrder"
          >
            {{ t('recharge.iPaid') }}
          </button>
          <button v-if="paid" class="btn primary" type="button" @click="close">
            {{ t('common.done') }}
          </button>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.recharge-modal {
  width: 460px;
}
.dialog-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 4px;
}
.dialog-head h2 {
  margin: 0;
  font-size: 17px;
  font-weight: 600;
}
.sub {
  margin: 4px 0 0;
}
.dialog-x {
  flex: none;
  width: 28px;
  height: 28px;
  margin: -4px -6px 0 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--muted);
  font-size: 20px;
  line-height: 1;
  cursor: pointer;
}
.dialog-x:hover {
  background: var(--accent-soft);
  color: var(--accent);
}
.label {
  display: block;
  margin: 18px 0 8px;
  font-size: 13px;
  color: var(--muted);
}
.presets {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 8px;
}
.preset {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 12px 0;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: #fff;
  cursor: pointer;
}
.preset:hover {
  border-color: var(--accent);
}
.preset.active {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-soft);
}
.preset-num {
  font-size: 17px;
  font-weight: 600;
}
.preset-unit {
  font-size: 12px;
  color: var(--muted);
}
.preset.active .preset-unit {
  color: var(--accent);
}
.input-wrap {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: #fff;
}
.input-wrap:focus-within {
  border-color: var(--accent);
}
.input-wrap .prefix {
  color: var(--muted);
  font-size: 14px;
}
.input-wrap input {
  flex: 1;
  padding: 10px 0;
  border: 0;
  outline: none;
  background: transparent;
  font-size: 14px;
}
.pay-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  margin-top: 18px;
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
  display: flex;
  align-items: baseline;
  gap: 8px;
  font-size: 20px;
  font-weight: 600;
}
.waiting {
  font-size: 12px;
  font-weight: 400;
  color: var(--muted);
}
.ok {
  color: var(--ok);
  font-weight: 600;
}
</style>
